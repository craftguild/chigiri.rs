//! Verifies that Rust version output starts without consulting Git, Cargo, or a manifest.

use std::process::Command;

fn cli(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_chigiri"))
        .arg("rust")
        .args(arguments)
        .current_dir(std::env::temp_dir())
        .env("PATH", "/chigiri-no-tools")
        .env("GIT_DIR", "/not-the-generator-repository")
        .env("HOST", "not-the-build-host")
        .output()
        .unwrap()
}

#[test]
fn root_and_explicit_version_commands_return_identical_build_information() {
    let expected = chigiri_codegen::generator_info();
    for command in [
        vec![],
        vec!["init"],
        vec!["codegen"],
        vec!["update"],
        vec!["check"],
        vec!["doctor"],
    ] {
        let mut arguments = vec!["--manifest", "missing.toml", "--format", "json"];
        arguments.extend(command);
        arguments.extend(["--deny-warnings", "--version"]);
        let output = cli(&arguments);
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let info = chigiri_codegen::read_generator_info(&output.stdout).unwrap();
        assert_eq!(&info, expected);
    }
}

#[test]
fn human_and_json_versions_expose_the_same_fields() {
    let output = cli(&["--version"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let banner = include_str!("../assets/version-banner.txt");
    assert!(text.starts_with(banner));
    assert!(text.contains("craftguild/chigiri.rs"));
    let info = chigiri_codegen::generator_info();
    for value in [
        &info.version,
        &info.build.rustc,
        &info.build.host,
        &info.build.target,
        &info.build.profile,
    ] {
        assert!(text.contains(value.as_ref()));
    }
    assert!(text.contains(info.git_commit.as_deref().unwrap_or("unknown")));
    if info.dirty == Some(true) {
        assert!(text.contains(" (dirty)"));
    }
    if info.dirty.is_none() {
        assert!(text.contains("dirty: unknown"));
    }
    assert!(!text.contains("not-the-build-host"));
}

#[test]
fn version_rejects_legacy_overrides_and_unrecognized_arguments() {
    for arguments in [
        vec!["--version", "--schema-dir", "graphql"],
        vec!["--version", "--config", "chigiri.toml"],
        vec!["--version", "--output-dir", "generated"],
        vec!["--version", "--diagnostic-format", "json"],
        vec!["--version", "--allow-lockfile-update"],
        vec!["--version", "--version"],
        vec!["--version", "--format", "yaml"],
        vec!["--version", "--manifest"],
        vec!["schema", "--version"],
        vec!["generate", "--version"],
        vec!["snapshot", "--version"],
        vec!["diff", "--version"],
        vec!["scaffold", "--version"],
    ] {
        let output = cli(&arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty());
    }
}
