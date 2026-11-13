//! Bounded observation of Git and compiler identity from the build root.

use std::borrow::Cow;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use chigiri_core::{GENERATOR_INFO_SCHEMA, GeneratorBuild, GeneratorInfo};

use crate::process::ProcessLimits;

pub(crate) fn capture(root: &Path) -> GeneratorInfo {
    let mut info = GeneratorInfo {
        schema_version: GENERATOR_INFO_SCHEMA.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        revision: "unknown".into(),
        git_commit: None,
        dirty: None,
        build: GeneratorBuild {
            rustc: "unknown".into(),
            host: token("HOST"),
            target: token("TARGET"),
            profile: token("PROFILE"),
        },
    };
    if let Some((commit, dirty)) = repository(root) {
        info.revision = if dirty {
            format!("{commit}-dirty").into()
        } else {
            commit.clone().into()
        };
        info.git_commit = Some(commit.into());
        info.dirty = Some(dirty);
    }
    if let Some(rustc) = std::env::var_os("RUSTC")
        && let Some(bytes) = output(Command::new(rustc).arg("--version").current_dir(root))
        && let Ok(banner) = String::from_utf8(bytes)
    {
        info.build.rustc = banner.trim().to_owned().into();
        if info.validate().is_err() {
            info.build.rustc = "unknown".into();
        }
    }
    info
}

fn token(name: &str) -> Cow<'static, str> {
    std::env::var(name)
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 256
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        .map_or_else(|| "unknown".into(), Into::into)
}

fn repository(root: &Path) -> Option<(String, bool)> {
    // An archive built in a consumer repository must not inherit an ancestor repository's HEAD.
    let entry = std::fs::symlink_metadata(root.join(".git")).ok()?;
    if !(entry.is_dir() || (entry.is_file() && entry.len() <= 4096))
        || [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return None;
    }
    let git = || {
        let mut command = Command::new("git");
        command
            .current_dir(root)
            .args(["--no-optional-locks", "--git-dir"])
            .arg(root.join(".git"))
            .arg("--work-tree")
            .arg(root);
        command
    };
    let head = output(git().args(["rev-parse", "--verify", "HEAD^{commit}"]))?;
    let commit = std::str::from_utf8(&head).ok()?.trim();
    if commit.len() != 40
        || !commit.bytes().any(|byte| byte != b'0')
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    // NUL-delimited status preserves non-UTF-8 filenames; command failure remains unknown, not clean.
    let status = output(git().args([
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=normal",
        "--ignore-submodules=none",
    ]))?;
    Some((commit.to_owned(), !status.is_empty()))
}

fn output(command: &mut Command) -> Option<Vec<u8>> {
    let output = crate::process::run(
        command,
        &[],
        ProcessLimits {
            timeout: Duration::from_secs(3),
            bytes: 256 * 1024,
        },
        // The build information observation does not introduce the process-global signal registration of CLI.
        || false,
    )
    .ok()?;
    output.status.success().then_some(output.stdout)
}

pub(crate) fn rust_literal(info: &GeneratorInfo) -> String {
    let text = |value: &str| format!("::std::borrow::Cow::Borrowed({value:?})");
    let commit = info.git_commit.as_ref().map_or_else(
        || "::std::option::Option::None".to_owned(),
        |value| format!("::std::option::Option::Some({})", text(value)),
    );
    let dirty = info.dirty.map_or_else(
        || "::std::option::Option::None".to_owned(),
        |value| format!("::std::option::Option::Some({value})"),
    );
    format!(
        "//buildSometimes observed formationCLIInformation. Do not retrieve from runtime.\n\
         static GENERATOR_INFO: ::chigiri_core::GeneratorInfo = ::chigiri_core::GeneratorInfo {{\n\
         schema_version: {}, version: {}, revision: {}, git_commit: {commit}, dirty: {dirty},\n\
         build: ::chigiri_core::GeneratorBuild {{ rustc: {}, host: {}, target: {}, profile: {} }},\n\
         }};\n",
        text(&info.schema_version),
        text(&info.version),
        text(&info.revision),
        text(&info.build.rustc),
        text(&info.build.host),
        text(&info.build.target),
        text(&info.build.profile),
    )
}
