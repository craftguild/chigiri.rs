//! Verify exclusive export outside the project and safe cleanup of verified files on disk.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::test_support::Fixture;
use super::{export_project, read_project, update_project};

const BLOG: &str = include_str!("../../fixtures/blog.graphql");

#[test]
fn mp_25_26_28_export_is_deterministic_external_and_excludes_project_management() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    fixture.write(
        "seeds/User.jsonl",
        "{\"name\":\"First\",\"id\":\"u1\"}\r\n{\"id\":\"u2\",\"name\":\"Second\"}",
    );
    fixture.write("resolvers/query/getUsers.luau.dist", "private proposal");
    fixture.write("resolvers/query/removed.luau", "private orphan");
    let output = fixture.base.join("bundle.toml");
    export_project(&fixture.project, &output).unwrap();
    let bytes = fs::read_to_string(&output).unwrap();
    assert_eq!(bytes, read_project(&fixture.project).unwrap().bundle());
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for forbidden in [
        "private proposal",
        "private orphan",
        "ChigiriMockProjectConfig",
        "update-state",
        "schema.snapshot",
        fixture.base.to_str().unwrap(),
    ] {
        assert!(!bytes.contains(forbidden), "{forbidden}");
    }
    let next = fixture.project.join("../second.toml");
    export_project(&fixture.project, &next).unwrap();
    assert_eq!(bytes, fs::read_to_string(next).unwrap());
    assert!(export_project(&fixture.project, &output).is_err());
    assert_eq!(bytes, fs::read_to_string(output).unwrap());
    for path in [
        "bundle.toml",
        "generated/bundle.toml",
        "resolvers/query/bundle.toml",
    ] {
        assert!(export_project(&fixture.project, &fixture.project.join(path)).is_err());
        assert!(!fixture.project.join(path).exists());
    }
}

#[test]
fn mp_28_export_late_creation_preserves_destination_and_uses_only_checked_bytes() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let output = fixture.base.join("late.toml");
    let error = super::export::export_with(&fixture.project, &output, || {
        fs::write(&output, "late user file").unwrap();
    })
    .err()
    .unwrap();
    assert!(!error.committed);
    assert!(error.recovery.is_empty());
    assert_eq!(fs::read_to_string(&output).unwrap(), "late user file");
    let expected = read_project(&fixture.project).unwrap().bundle().to_owned();
    let output = fixture.base.join("checked.toml");
    super::export::export_with(&fixture.project, &output, || {
        // Changes made after the completed readiness check cannot enter the already owned export snapshot.
        fixture.write("seeds/User.jsonl", "changed after completed check");
        fixture.write(
            "resolvers/query/getUsers.luau",
            "changed after completed check",
        );
    })
    .unwrap();
    assert_eq!(fs::read_to_string(output).unwrap(), expected);
    assert!(read_project(&fixture.project).is_err());
}

#[test]
fn mp_28_export_rejects_modified_temporary_file_and_preserves_recovery_material() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let output = fixture.base.join("must-not-publish.toml");
    let mut temporary = None;
    let error = super::export::export_with(&fixture.project, &output, || {
        let path = fs::read_dir(&fixture.base)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".chigiri-mock-export-")
            })
            .unwrap();
        fs::write(&path, "edited temporary").unwrap();
        temporary = Some(path);
    })
    .err()
    .unwrap();
    assert!(!error.committed);
    assert!(!error.recovery.is_empty());
    assert!(!output.exists());
    assert_eq!(
        fs::read_to_string(temporary.unwrap()).unwrap(),
        "edited temporary"
    );
}

#[test]
fn mp_28_export_accepts_scaffolds_and_rejects_symlink_inputs_without_implicit_update() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Query { answer:Int! @chigiri_mock_resolver }");
    let before = fixture.read("generated/update-state.json");
    let output = fixture.base.join("scaffold.toml");
    export_project(&fixture.project, &output).unwrap();
    assert!(output.exists());
    assert_eq!(before, fixture.read("generated/update-state.json"));
    fs::remove_file(&output).unwrap();
    std::os::unix::fs::symlink(&fixture.base, fixture.base.join("alias")).unwrap();
    assert!(export_project(&fixture.project, &fixture.base.join("alias/out.toml")).is_err());
    std::os::unix::fs::symlink("absent", &output).unwrap();
    assert!(export_project(&fixture.project, &output).is_err());
    assert!(
        fs::symlink_metadata(output)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[tokio::test]
async fn tl_13_fixed_clock_is_generation_owned_and_preserved_by_portable_export() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let config = fixture.read("mock.luau").replace(
        "clock = {mode = \"system\"}",
        "clock = {mode = \"fixed\", now = 1789776000}",
    );
    fixture.write("mock.luau", &config);
    update_project(&fixture.project).unwrap();
    let state: serde_json::Value =
        serde_json::from_str(&fixture.read("generated/update-state.json")).unwrap();
    assert_eq!(state["config"]["clock_mode"], "fixed");
    assert_eq!(state["config"]["clock_now"], 1789776000_i64);
    assert_eq!(
        state["config"]["capability_version"],
        "chigiri-mock-capabilities/v4"
    );
    let checked = read_project(&fixture.project).unwrap();
    let bundle = checked.bundle().to_owned();
    assert!(bundle.contains("[clock]\nmode = \"fixed\""));
    assert!(bundle.contains("now = 1789776000"));
    let crate::checked::RuntimeInputs {
        scripts,
        hooks,
        store,
        clock,
        ..
    } = checked.into_runtime();
    assert_eq!(clock.mode(), "fixed");
    assert_eq!(clock.fixed_value(), Some(1789776000));
    let mut worker =
        crate::service::Worker::from_checked_with_clock(scripts, hooks, store, clock).unwrap();
    worker
        .control()
        .run_luau("timeleap.travel_to(1789776001,{world=true})")
        .unwrap();
    worker
        .control()
        .run_luau("timeleap.travel_to(1789776002,{world=true})")
        .unwrap();
    assert_eq!(
        worker.control().policy(),
        crate::clock::Policy::Fixed(1789776002)
    );
    assert_eq!(read_project(&fixture.project).unwrap().bundle(), bundle);
    let changed = fixture
        .read("mock.luau")
        .replace("now = 1789776000", "now = 1789777000");
    fixture.write("mock.luau", &changed);
    update_project(&fixture.project).unwrap();
    worker
        .control()
        .run_luau("timeleap.reset({world=true})")
        .unwrap();
    assert_eq!(
        worker.control().policy(),
        crate::clock::Policy::Fixed(1789776000)
    );
    let restarted = read_project(&fixture.project).unwrap();
    let crate::checked::RuntimeInputs {
        scripts,
        hooks,
        store,
        clock,
        ..
    } = restarted.into_runtime();
    let mut next =
        crate::service::Worker::from_checked_with_clock(scripts, hooks, store, clock).unwrap();
    assert_eq!(
        next.control().policy(),
        crate::clock::Policy::Fixed(1789777000)
    );
    next.stop(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
        .unwrap();
    worker
        .stop(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
        .unwrap();
}
