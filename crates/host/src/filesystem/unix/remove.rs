//! Atomically claims a named entry in a private directory before destruction.

use std::ffi::{OsStr, OsString};
use std::io::Read;

use rustix::fs::{self, AtFlags, Mode, RenameFlags, Stat};

use super::directory::Identity;
use super::{Directory, FsError, os_error};

const CLAIM_ENTRY: &str = "entry";

struct Claim {
    directory: Directory,
    name: OsString,
}

pub(super) fn file(
    parent: &Directory,
    name: &OsStr,
    expected: &Stat,
    after_claim: impl FnOnce(),
) -> Result<(), FsError> {
    let claim = claim(parent, name)?;
    after_claim();
    let found = match fs::statat(
        claim.directory.descriptor(),
        CLAIM_ENTRY,
        AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Ok(found) => found,
        Err(error) => {
            restore(parent, name, claim);
            return Err(os_error(error));
        }
    };
    if !same_file_claim(expected, &found) {
        restore(parent, name, claim);
        return Err(FsError::Changed);
    }
    match fs::unlinkat(claim.directory.descriptor(), CLAIM_ENTRY, AtFlags::empty()) {
        Ok(()) => finish(parent, claim),
        Err(error) => {
            restore(parent, name, claim);
            Err(os_error(error))
        }
    }
}

fn same_file_claim(expected: &Stat, found: &Stat) -> bool {
    Identity::from_stat(expected) == Identity::from_stat(found)
        && expected.st_size == found.st_size
        && expected.st_mtime == found.st_mtime
        && expected.st_mtime_nsec == found.st_mtime_nsec
}

pub(super) fn directory(
    parent: &Directory,
    name: &OsStr,
    expected: Identity,
) -> Result<(), FsError> {
    let claim = claim(parent, name)?;
    let found = match fs::statat(
        claim.directory.descriptor(),
        CLAIM_ENTRY,
        AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Ok(found) => found,
        Err(error) => {
            restore(parent, name, claim);
            return Err(os_error(error));
        }
    };
    if Identity::from_stat(&found) != expected {
        restore(parent, name, claim);
        return Err(FsError::Changed);
    }
    match fs::unlinkat(
        claim.directory.descriptor(),
        CLAIM_ENTRY,
        AtFlags::REMOVEDIR,
    ) {
        Ok(()) => finish(parent, claim),
        Err(error) => {
            restore(parent, name, claim);
            Err(os_error(error))
        }
    }
}

fn claim(parent: &Directory, name: &OsStr) -> Result<Claim, FsError> {
    parent.verify()?;
    for _ in 0..32 {
        let private_name = random_name(".chigiri-remove-")?;
        match fs::mkdirat(
            parent.descriptor(),
            &private_name,
            Mode::RUSR | Mode::WUSR | Mode::XUSR,
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) => continue,
            Err(error) => return Err(os_error(error)),
        }
        let private = match parent.child(&private_name) {
            Ok(private) => private,
            Err(error) => {
                let _ = fs::unlinkat(parent.descriptor(), &private_name, AtFlags::REMOVEDIR);
                return Err(error);
            }
        };
        match fs::renameat_with(
            parent.descriptor(),
            name,
            private.descriptor(),
            CLAIM_ENTRY,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {
                let claim = Claim {
                    directory: private,
                    name: private_name,
                };
                if let Err(error) = parent.sync() {
                    restore(parent, name, claim);
                    return Err(error);
                }
                return Ok(claim);
            }
            Err(error) => {
                let _ = fs::unlinkat(parent.descriptor(), &private_name, AtFlags::REMOVEDIR);
                return Err(if error == rustix::io::Errno::NOENT {
                    FsError::Changed
                } else {
                    os_error(error)
                });
            }
        }
    }
    Err(FsError::Busy)
}

fn finish(parent: &Directory, claim: Claim) -> Result<(), FsError> {
    claim.directory.sync()?;
    fs::unlinkat(parent.descriptor(), &claim.name, AtFlags::REMOVEDIR).map_err(os_error)?;
    parent.verify()?;
    parent.sync()
}

fn restore(parent: &Directory, name: &OsStr, claim: Claim) {
    match fs::renameat_with(
        claim.directory.descriptor(),
        CLAIM_ENTRY,
        parent.descriptor(),
        name,
        RenameFlags::NOREPLACE,
    ) {
        Ok(()) => {
            let _ = finish(parent, claim);
        }
        Err(rustix::io::Errno::EXIST) => restore_around_replacement(parent, name, claim),
        Err(_) => {}
    }
}

fn restore_around_replacement(parent: &Directory, name: &OsStr, claim: Claim) {
    if fs::renameat_with(
        claim.directory.descriptor(),
        CLAIM_ENTRY,
        parent.descriptor(),
        name,
        RenameFlags::EXCHANGE,
    )
    .is_err()
    {
        return;
    }
    for _ in 0..32 {
        let Ok(recovery) = random_name(".chigiri-recovered-") else {
            return;
        };
        match fs::renameat_with(
            claim.directory.descriptor(),
            CLAIM_ENTRY,
            parent.descriptor(),
            &recovery,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {
                let _ = finish(parent, claim);
                return;
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(_) => return,
        }
    }
}

pub(super) fn random_name(prefix: &str) -> Result<OsString, FsError> {
    let mut entropy = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut entropy))
        .map_err(|_| FsError::Io)?;
    let mut name = String::with_capacity(prefix.len() + entropy.len() * 2);
    name.push_str(prefix);
    for byte in entropy {
        use std::fmt::Write;
        write!(name, "{byte:02x}").map_err(|_| FsError::Io)?;
    }
    Ok(name.into())
}
