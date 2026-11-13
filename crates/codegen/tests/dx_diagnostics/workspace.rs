//! Verify explicit consumer selection, ambiguous input, and rejection of symlinked metadata.

use std::fs;

use super::doctor_fixture::{lock, prepare, tool_log};
use super::fixture::{Fixture, report};

#[test]
fn diag_06_rejects_unspecified_packages_in_virtual_workspaces() {
    let fixture = workspace();
    let path = fixture.root.join("selected.toml");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        source.replace("package=\"sibling-doctor-consumer\"\n", ""),
    )
    .unwrap();
    let output = fixture.run("doctor");
    assert_eq!(output.status.code(), Some(1));
    assert!(report(&output).to_string().contains("SF4011"));
    fs::write(
        &path,
        source.replace("package=\"sibling-doctor-consumer\"", "package=\"absent\""),
    )
    .unwrap();
    fixture.success("update");
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(report(&output).to_string().contains("SF4015"));
    assert_eq!(fs::read_to_string(log).unwrap(), "metadata\n");
}

#[test]
fn diag_06_virtual_workspace_only_select_the_specified_package() {
    let fixture = workspace();
    let output = fixture.run("doctor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = report(&output).to_string();
    assert!(text.contains("--package sibling-doctor-consumer"));
    assert!(text.contains("--manifest-path consumer/member/Cargo.toml"));
}

#[test]
fn diag_06_rejects_manifest_symlinks_and_package_overrides() {
    let fixture = prepare("");
    let mismatch = fixture.execute(fixture.command("doctor").args(["--package", "other"]));
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(report(&mismatch).to_string().contains("SF4001"));
    fs::rename(
        fixture.root.join("consumer/Cargo.toml"),
        fixture.root.join("consumer/actual.toml"),
    )
    .unwrap();
    std::os::unix::fs::symlink("actual.toml", fixture.root.join("consumer/Cargo.toml")).unwrap();
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let linked = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert_eq!(linked.status.code(), Some(1));
    assert!(report(&linked).to_string().contains("SF4015"));
    assert!(!log.exists());
}

fn workspace() -> Fixture {
    let fixture = prepare("");
    let consumer = fixture.root.join("consumer");
    let member = consumer.join("member");
    fs::create_dir(&member).unwrap();
    fs::rename(consumer.join("src"), member.join("src")).unwrap();
    let manifest = fs::read_to_string(consumer.join("Cargo.toml"))
        .unwrap()
        .replace("[workspace]\n", "")
        .replace("../generated/contract", "../../generated/contract")
        .replace("../../../../crates/facade", "../../../../../crates/facade");
    fs::write(member.join("Cargo.toml"), manifest).unwrap();
    fs::write(
        consumer.join("Cargo.toml"),
        "[workspace]\nmembers=[\"member\"]\nresolver=\"2\"\n",
    )
    .unwrap();
    lock(&fixture);
    fixture
}
