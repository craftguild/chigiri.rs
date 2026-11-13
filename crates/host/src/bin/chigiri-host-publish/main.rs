//! Native atomic publication primitive used by distribution builds.

#![forbid(unsafe_code)]

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod supported {
    use std::ffi::{OsStr, OsString};
    use std::os::fd::OwnedFd;
    use std::path::{Component, Path};

    use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags, Stat};

    const CANDIDATE: &str = "candidate";

    struct Chain {
        descriptors: Vec<OwnedFd>,
        names: Vec<OsString>,
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    struct Identity {
        device: u64,
        inode: u64,
        kind: FileType,
    }

    impl Identity {
        #[allow(clippy::unnecessary_cast)]
        fn from_stat(found: &Stat) -> Self {
            Self {
                device: found.st_dev as u64,
                inode: found.st_ino as u64,
                kind: FileType::from_raw_mode(found.st_mode),
            }
        }

        fn regular(self) -> Result<Self, String> {
            if self.kind == FileType::RegularFile {
                Ok(self)
            } else {
                Err("publication entry is not a regular file".into())
            }
        }
    }

    impl Chain {
        fn open(path: &Path) -> Result<Self, String> {
            if !path.is_absolute() {
                return Err("publication directories must be absolute".into());
            }
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let mut descriptors =
                vec![fs::open("/", flags, Mode::empty()).map_err(|error| error.to_string())?];
            let mut names = Vec::new();
            for component in path.components() {
                match component {
                    Component::RootDir => {}
                    Component::Normal(name) => {
                        let next = fs::openat(
                            descriptors.last().expect("root descriptor is retained"),
                            name,
                            flags,
                            Mode::empty(),
                        )
                        .map_err(|error| error.to_string())?;
                        descriptors.push(next);
                        names.push(name.to_owned());
                    }
                    _ => return Err("publication directory has an invalid component".into()),
                }
            }
            let chain = Self { descriptors, names };
            chain.verify()?;
            Ok(chain)
        }

        fn directory(&self) -> &OwnedFd {
            self.descriptors
                .last()
                .expect("root descriptor is retained")
        }

        fn verify(&self) -> Result<(), String> {
            for ((parent, child), name) in self
                .descriptors
                .iter()
                .zip(self.descriptors.iter().skip(1))
                .zip(&self.names)
            {
                let retained =
                    Identity::from_stat(&fs::fstat(child).map_err(|error| error.to_string())?);
                let named = Identity::from_stat(
                    &fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
                        .map_err(|_| "publication directory ancestry changed".to_string())?,
                );
                if retained != named || named.kind != FileType::Directory {
                    return Err("publication directory ancestry changed".into());
                }
            }
            Ok(())
        }

        fn sync(&self) -> Result<(), String> {
            fs::fsync(self.directory()).map_err(|error| error.to_string())
        }
    }

    fn named(chain: &Chain, name: &OsStr) -> Result<Identity, String> {
        fs::statat(chain.directory(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map(|found| Identity::from_stat(&found))
            .map_err(|error| error.to_string())
    }

    fn exchange(private: &Chain, slot: &Chain, name: &OsStr) -> Result<(), String> {
        fs::renameat_with(
            private.directory(),
            CANDIDATE,
            slot.directory(),
            name,
            RenameFlags::EXCHANGE,
        )
        .map_err(|error| error.to_string())
    }

    fn rollback_exchange(private: &Chain, slot: &Chain, name: &OsStr, retained: Identity) {
        let safe = named(private, OsStr::new(CANDIDATE)) == Ok(retained)
            && exchange(private, slot, name).is_ok()
            && named(slot, name) == Ok(retained)
            && private.verify().is_ok()
            && slot.verify().is_ok()
            && private.sync().is_ok()
            && slot.sync().is_ok();
        if !safe {
            std::process::abort();
        }
    }

    fn install(private: &Chain, slot: &Chain, name: &OsStr) -> Result<(), String> {
        fs::renameat_with(
            private.directory(),
            CANDIDATE,
            slot.directory(),
            name,
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| error.to_string())
    }

    fn rollback_install(private: &Chain, slot: &Chain, name: &OsStr, candidate: Identity) {
        let safe = named(slot, name) == Ok(candidate)
            && fs::renameat_with(
                slot.directory(),
                name,
                private.directory(),
                CANDIDATE,
                RenameFlags::NOREPLACE,
            )
            .is_ok()
            && named(private, OsStr::new(CANDIDATE)) == Ok(candidate)
            && private.sync().is_ok()
            && slot.sync().is_ok();
        if !safe {
            std::process::abort();
        }
    }

    fn validate_name(name: &OsStr) -> Result<(), String> {
        let mut components = Path::new(name).components();
        if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
            Ok(())
        } else {
            Err("artifact name must be one path component".into())
        }
    }

    fn argument_identity(
        arguments: &[OsString],
        offset: usize,
        label: &str,
    ) -> Result<Identity, String> {
        let parse = |index: usize| {
            arguments[index]
                .to_str()
                .ok_or_else(|| format!("{label} identity is not UTF-8"))?
                .parse()
                .map_err(|_| format!("{label} identity is invalid"))
        };
        Ok(Identity {
            device: parse(offset)?,
            inode: parse(offset + 1)?,
            kind: FileType::RegularFile,
        })
    }

    fn publish_with_hook(
        arguments: &[OsString],
        after_observe: impl FnOnce(),
    ) -> Result<(), String> {
        let action = arguments
            .first()
            .and_then(|value| value.to_str())
            .ok_or("publication action is absent or is not UTF-8")?;
        if !((action == "install" && arguments.len() == 6)
            || (action == "exchange" && arguments.len() == 8))
        {
            return Err(
                "usage: chigiri-host-publish <install|exchange> <private-dir> <slot-dir> \
                 <name> <candidate-device> <candidate-inode> \
                 [<previous-device> <previous-inode>]"
                    .into(),
            );
        }
        let private = Chain::open(Path::new(&arguments[1]))?;
        let slot = Chain::open(Path::new(&arguments[2]))?;
        let name = &arguments[3];
        validate_name(name)?;
        let expected = argument_identity(arguments, 4, "candidate")?;
        let candidate = named(&private, OsStr::new(CANDIDATE))?.regular()?;
        if candidate != expected {
            return Err("publication candidate identity changed".into());
        }
        private.verify()?;
        slot.verify()?;
        match action {
            "install" => {
                install(&private, &slot, name)?;
                if let Err(error) = (|| {
                    if named(&slot, name)? != candidate {
                        return Err("published candidate identity changed".into());
                    }
                    private.verify()?;
                    slot.verify()?;
                    slot.sync()
                })() {
                    rollback_install(&private, &slot, name, candidate);
                    return Err(error);
                }
            }
            "exchange" => {
                let expected_previous = argument_identity(arguments, 6, "previous")?;
                let previous = named(&slot, name)?.regular()?;
                if previous != expected_previous {
                    return Err("existing publication identity changed".into());
                }
                after_observe();
                exchange(&private, &slot, name)?;
                let retained = named(&private, OsStr::new(CANDIDATE))
                    .unwrap_or_else(|_| std::process::abort());
                if let Err(error) = (|| {
                    if named(&slot, name)? != candidate || retained != expected_previous {
                        return Err("atomic publication identities changed".into());
                    }
                    private.verify()?;
                    slot.verify()?;
                    private.sync()?;
                    slot.sync()
                })() {
                    rollback_exchange(&private, &slot, name, retained);
                    return Err(error);
                }
            }
            _ => return Err("publication action must be install or exchange".into()),
        }
        Ok(())
    }

    fn publish(arguments: &[OsString]) -> Result<(), String> {
        publish_with_hook(arguments, || {})
    }

    pub(super) fn run() -> Result<(), String> {
        publish(&std::env::args_os().skip(1).collect::<Vec<_>>())
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    if let Err(error) = supported::run() {
        eprintln!("atomic publication failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("atomic publication is unsupported on this build host");
    std::process::exit(1);
}
