//! Test new-project layout, existing-file protection, and cleanup during concurrent changes.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::filesystem::Directory;
use super::staging::Staging;
use super::test_support::Fixture;
use super::{ProjectError, init_project, read_project, read_project_schema};

const BLOG: &str = include_str!("../../fixtures/blog.graphql");

#[test]
fn mp_01_06_init_places_deterministic_private_project_with_explicit_external_sources() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let path = fixture.base.join("new-project");
    let sources = [fixture.base.join("schema.graphql")];
    init_project(&sources, &path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path.join("mock.luau"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let schema = read_project_schema(&path).unwrap();
    assert_eq!(schema.settings().sources[0], "../schema.graphql");
    let adopted = schema.ir().graph().source().sources()[0].text();
    assert!(adopted.starts_with("type User {"));
    assert!(adopted.ends_with(" }\n"));
    assert!(!adopted.contains("# Blog"));
    read_project(&path).unwrap();
    let config = fs::read_to_string(path.join("mock.luau")).unwrap();
    assert!(config.contains("events = {"));
    assert_eq!(
        schema.settings().events,
        crate::events::EventSettings::default()
    );
    assert!(!config.contains("type User"));
    assert!(!path.join("schema.graphql").exists());
    let guidance = fs::read_to_string(path.join("AGENTS.md")).unwrap();
    assert!(guidance.contains("chigiri mock reference types Todo"));
    assert!(guidance.contains("chigiri mock reference runtime storage"));
    assert!(guidance.contains("chigiri mock reference runtime stdlib"));
    assert!(guidance.contains("chigiri mock reference runtime timeleap"));
    assert!(guidance.contains("reference hooks request.before"));
    assert!(guidance.contains("NOT_IMPLEMENTED"));
    assert!(guidance.contains("ctx.pubsub.subscribe.Item"));
    assert!(guidance.contains("ctx.pubsub.publish.Item"));
    assert!(guidance.contains("diagnostic's `references` list"));
    assert!(guidance.contains("0 through\n9,007,199,254,740,991 inclusive"));
    assert!(guidance.contains("There is no `ctx.clock` compatibility API."));
    assert!(guidance.contains("`storage.graphql` for private models"));
    for expected in [
        "Stored object fields are snapshots",
        "stable embedded `Image`",
        "changing `Wallet`",
        "Return model records unchanged",
        "Arbitrary output DTOs may contain public fields only",
        "MOCK_OUTPUT_PRIVATE_FIELD",
        "Project-local `require` and shared Luau modules are unsupported",
        "`type(request) == \"table\"`",
        "guard with `return error(...)`",
        "`ctx.request` is already available",
        "hooks are optional",
        "Seed `$ref` values copy one validated snapshot",
        "mock-only public-field override",
        "then adopt it with `update`",
        "Target checking, full readiness, and an\nactual HTTP scenario",
    ] {
        assert!(guidance.contains(expected), "{expected}");
    }
    let edit = guidance.find("2. Edit `storage.graphql`").unwrap();
    let codegen = guidance
        .find("3. After changing private SDL or generation-bound settings")
        .unwrap();
    let reference = guidance
        .find("4. Inspect `generated/types.d.luau`")
        .unwrap();
    let implement = guidance.find("5. Edit the smallest necessary set").unwrap();
    let target = guidance.find("6. Run a target check").unwrap();
    let full = guidance.find("7. Run a full check").unwrap();
    assert!(edit < codegen && codegen < reference && reference < implement);
    assert!(implement < target && target < full);
    assert!(guidance.contains("chigiri mock codegen --config-dir ."));
    assert!(guidance.contains("chigiri mock update --config-dir . --check"));
    assert!(guidance.contains("exercise representative GraphQL HTTP scenarios"));
    assert!(guidance.contains("Only a successful check without `--target`"));
    assert!(!guidance.contains(&sources[0].to_string_lossy().to_string()));
    let other = fixture.base.join("other-project");
    init_project(&sources, &other).unwrap();
    assert_eq!(
        guidance,
        fs::read_to_string(other.join("AGENTS.md")).unwrap()
    );
    for item in super::generate_project(schema.ir().clone())
        .unwrap()
        .files
        .keys()
    {
        assert_eq!(
            fs::read(path.join(item.as_str())).unwrap(),
            fs::read(other.join(item.as_str())).unwrap()
        );
    }
    assert!(init_project(&sources, &path).is_err());
    assert_eq!(fs::read_to_string(path.join("mock.luau")).unwrap(), config);
}

#[test]
fn cv_01_init_manual_scaffold_passes_full_validation_without_execution() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Query { answer: Int! @chigiri_mock_resolver }");
    let path = fixture.base.join("manual");
    init_project(&[fixture.base.join("schema.graphql")], &path).unwrap();
    let checked = read_project(&path).unwrap();
    assert!(
        checked.scripts().scripts()[0]
            .source()
            .contains("NOT_IMPLEMENTED")
    );
}

#[test]
fn mp_13_15_init_late_destination_and_source_changes_leave_user_files_intact() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let path = fixture.base.join("late");
    let sources = [fixture.base.join("schema.graphql")];
    let result = super::init::init_with(&sources, &path, || {
        fs::create_dir(&path).unwrap();
        fs::write(path.join("user.txt"), "preserved").unwrap();
    });
    assert!(!result.err().unwrap().committed);
    assert_eq!(
        fs::read_to_string(path.join("user.txt")).unwrap(),
        "preserved"
    );
    assert!(!path.join("mock.luau").exists());
    let path = fixture.base.join("source-changed");
    let error = super::init::init_with(&sources, &path, || {
        fs::write(&sources[0], "changed SDL").unwrap();
    })
    .err()
    .unwrap();
    assert_eq!(error.message, "input changed during command");
    assert!(!path.exists());
    assert!(fs::read_dir(&fixture.base).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".chigiri-mock-init-")
    }));
}

#[test]
fn mp_13_init_rejects_symlink_parents_destinations_and_sources() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let sources = [fixture.base.join("schema.graphql")];
    std::os::unix::fs::symlink(&fixture.base, fixture.base.join("alias")).unwrap();
    assert!(init_project(&sources, &fixture.base.join("alias/new")).is_err());
    std::os::unix::fs::symlink(&fixture.project, fixture.base.join("destination")).unwrap();
    assert!(init_project(&sources, &fixture.base.join("destination")).is_err());
    std::os::unix::fs::symlink(&sources[0], fixture.base.join("link.graphql")).unwrap();
    assert!(
        init_project(
            &[fixture.base.join("link.graphql")],
            &fixture.base.join("new")
        )
        .is_err()
    );
    assert!(!fixture.base.join("new").exists());
}

#[test]
fn mp_15_private_cleanup_preserves_modified_and_unmanaged_files_and_attempts_all_targets() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(BLOG);
    let parent = Directory::open(&fixture.base).unwrap();
    let mut stage = Staging::new(&parent).unwrap();
    stage
        .write(&BTreeMap::from([
            (
                chigiri_host::PortableRelativePath::new("a/changed.txt").unwrap(),
                "original".into(),
            ),
            (
                chigiri_host::PortableRelativePath::new("b/owned.txt").unwrap(),
                "original".into(),
            ),
        ]))
        .unwrap();
    let stage_path = fs::read_dir(&fixture.base)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".chigiri-mock-init-")
        })
        .unwrap();
    fs::write(stage_path.join("a/changed.txt"), "user edit").unwrap();
    fs::write(stage_path.join("foreign.txt"), "user file").unwrap();
    let mut primary = ProjectError::new("init", "placement failed");
    stage.cleanup(&mut primary);
    assert!(primary.recovery.len() >= 2);
    assert_eq!(
        fs::read_to_string(stage_path.join("a/changed.txt")).unwrap(),
        "user edit"
    );
    assert_eq!(
        fs::read_to_string(stage_path.join("foreign.txt")).unwrap(),
        "user file"
    );
    assert!(!stage_path.join("b").exists());
    assert!(
        stage
            .publish(&parent, OsStr::new("must-not-publish"))
            .is_err()
    );
    assert!(!fixture.base.join("must-not-publish").exists());
}
