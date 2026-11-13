//! Fixtures where users and consumers create lockfiles through Cargo.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::fixture::Fixture;

pub(super) const CONSUMER: &str = include_str!("doctor_consumer.rs");

pub(super) fn prepare(settings: &str) -> Fixture {
    let fixture = Fixture::new();
    let path = fixture.root.join("selected.toml");
    let mut manifest = fs::read_to_string(&path).unwrap();
    manifest.push_str(&format!("[consumer]\ncargo_manifest=\"consumer/Cargo.toml\"\npackage=\"sibling-doctor-consumer\"\ngenerated_dependency=\"contract\"\n{settings}\n"));
    fs::write(path, manifest).unwrap();
    fs::create_dir_all(fixture.root.join("consumer/src")).unwrap();
    fs::write(fixture.root.join("consumer/src/lib.rs"), CONSUMER).unwrap();
    fs::write(fixture.root.join("consumer/Cargo.toml"), concat!(
        "[package]\nname=\"sibling-doctor-consumer\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
        "[features]\ndefault=[\"default-marker\"]\ndefault-marker=[]\nselected=[]\n",
        "[dependencies]\ncontract={package=\"sibling-cli-contract\",path=\"../generated/contract\"}\n",
        "chigiri={path=\"../../../../crates/facade\"}\n"
    )).unwrap();
    fixture.success("codegen");
    lock(&fixture);
    fixture
}

pub(super) fn lock(fixture: &Fixture) {
    let output = fixture.execute(
        Command::new(env!("CARGO"))
            .current_dir(&fixture.root)
            .args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(fixture.root.join("consumer/Cargo.toml")),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn inputs(fixture: &Fixture) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut directories = vec![fixture.root.join("consumer"), fixture.root.join("graphql")];
    files.insert(
        Path::new("selected.toml").to_owned(),
        fs::read(fixture.root.join("selected.toml")).unwrap(),
    );
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else {
                files.insert(
                    path.strip_prefix(&fixture.root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

pub(super) fn tool_log(fixture: &Fixture) -> std::ffi::OsString {
    fixture.wrapper("cargo", "#!/bin/sh\n# Record the phase before delegating to real Cargo.\nprintf '%s\\n' \"$1\" >> \"$CHIGIRI_TEST_CARGO_LOG\"\nexec \"$CHIGIRI_TEST_REAL_CARGO\" \"$@\"\n")
}
