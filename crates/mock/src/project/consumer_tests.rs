//! Registered module generation, checking and portable capture share one inventory.

use super::test_support::Fixture;
use super::{ActiveKind, ActiveSnapshot, Readiness, codegen_project, read_project};
use crate::luau::render_consumer_module;

const SDL: &str = "schema{query:Read mutation:Write subscription:Signals} type Read{ping:Int!} type Write{emit:Int!} type Signals{count(required:ID!):Int! other:Int!}";
const EVENTS: &str = "{limits={max_queued_events=1,max_queued_bytes=256,max_retained_bytes=4096,max_sequence_bytes=2048},consumers={audit={field='count',enabled=false},bridge={field='other',enabled=true}}}";

fn configure(fixture: &Fixture, base: &str, events: &str) {
    fixture.write(
        "mock.luau",
        &base.replace(
            "return config",
            &format!("config.events = {events}\nreturn config"),
        ),
    );
}

fn authored(snapshot: &ActiveSnapshot, name: &str, subscribe: &str, consume: &str) -> String {
    let registration = snapshot
        .schema()
        .settings()
        .events
        .resolve(snapshot.schema().ir())
        .unwrap()
        .into_iter()
        .find(|entry| entry.name() == name)
        .unwrap();
    render_consumer_module(
        snapshot.schema().ir(),
        registration.field(),
        subscribe,
        consume,
    )
    .unwrap()
}

#[test]
fn ev_05_codegen_captures_all_registered_modules_and_preserves_authored_bodies() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    configure(&fixture, &fixture.read("mock.luau"), EVENTS);
    codegen_project(&fixture.project).unwrap();
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert_eq!(
        snapshot
            .files()
            .iter()
            .filter(|file| matches!(file.kind(), ActiveKind::Consumer(_)))
            .count(),
        2
    );
    for name in ["audit", "bridge"] {
        let source = fixture.read(&format!("consumers/{name}.luau"));
        assert!(source.contains("NOT_IMPLEMENTED"));
        assert!(!source.contains("Args"));
    }
    let source = authored(
        &snapshot,
        "audit",
        "return ctx.pubsub.subscribe.Int({all_topics=true,overflow='drop_oldest'})",
        "ctx.log('info','received') ctx.pubsub.publish.Int({topic='out',payload=event})",
    );
    fixture.write("consumers/audit.luau", &source);
    // Codegen uses the adopted public SDL even when acquisition has changed.
    std::fs::write(fixture.base.join("schema.graphql"), "invalid external SDL").unwrap();
    codegen_project(&fixture.project).unwrap();
    assert_eq!(fixture.read("consumers/audit.luau"), source);
    let checked = read_project(&fixture.project).unwrap();
    assert_eq!(checked.consumers().len(), 2);
    assert!(!checked.consumers()[0].registration().enabled());
    assert!(checked.consumers()[1].registration().enabled());
    assert_eq!(checked.events().limits().max_queued_events(), 1);
}

#[test]
fn ev_05_disabled_consumer_bodies_are_checked_by_full_and_scoped_validation() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    configure(&fixture, &fixture.read("mock.luau"), EVENTS);
    codegen_project(&fixture.project).unwrap();
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let valid = authored(
        &snapshot,
        "audit",
        "error('subscribe must not execute during check')",
        "error('consume must not execute during check')",
    );
    fixture.write("consumers/audit.luau", &valid);
    read_project(&fixture.project).unwrap();
    let invalid = authored(
        &snapshot,
        "audit",
        "return ctx.pubsub.subscribe.Int({topic='x'})",
        "ctx.generator.sequence.Int({values={event},interval_ms=1})",
    );
    fixture.write("consumers/audit.luau", &invalid);
    let failure = read_project(&fixture.project).err().unwrap();
    assert_eq!(failure.phase, "type");
    assert_eq!(failure.file.as_str(), "consumers/audit.luau");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert!(matches!(
        snapshot
            .validate_selection(&[("consumers/audit.luau".into(), valid)])
            .unwrap(),
        Readiness::Ready
    ));
    assert_eq!(fixture.read("consumers/audit.luau"), invalid);
}

#[test]
fn ev_05_12_portable_capture_keeps_settings_disabled_sources_and_closed_coverage() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    configure(&fixture, &fixture.read("mock.luau"), EVENTS);
    codegen_project(&fixture.project).unwrap();
    let checked = read_project(&fixture.project).unwrap();
    let bundle = checked.bundle().to_owned();
    let expected = checked
        .consumers()
        .iter()
        .map(|consumer| {
            (
                consumer.registration().name().to_owned(),
                consumer.source().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let events = checked.events().clone();
    drop(checked);
    std::fs::remove_dir_all(&fixture.project).unwrap();
    std::fs::remove_file(fixture.base.join("schema.graphql")).unwrap();
    let path = std::path::Path::new("detached/consumer-bundle.toml");
    let parsed = crate::config::Config::parse(path, &bundle).unwrap();
    assert_eq!(parsed.checked.events(), &events);
    assert_eq!(
        parsed
            .checked
            .consumers()
            .iter()
            .map(|consumer| (
                consumer.registration().name().to_owned(),
                consumer.source().to_owned()
            ))
            .collect::<Vec<_>>(),
        expected
    );
    let mut missing = bundle.parse::<toml_edit::DocumentMut>().unwrap();
    missing["consumers"].as_table_mut().unwrap().remove("audit");
    assert!(crate::config::Config::parse(path, &missing.to_string()).is_err());
    let mut unknown = bundle.parse::<toml_edit::DocumentMut>().unwrap();
    unknown["consumers"]["unregistered"]["luau"] = toml_edit::value("return nil");
    assert!(crate::config::Config::parse(path, &unknown.to_string()).is_err());
}

#[test]
fn ev_05_removed_consumers_preserve_orphans_and_block_path_reuse_until_cleanup() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let base = fixture.read("mock.luau");
    configure(
        &fixture,
        &base,
        "{consumers={audit={field='count',enabled=false}}}",
    );
    codegen_project(&fixture.project).unwrap();
    fixture.write("consumers/audit.luau", "invalid orphan bytes !\n");
    configure(&fixture, &base, "{}");
    codegen_project(&fixture.project).unwrap();
    assert_eq!(
        fixture.read("consumers/audit.luau"),
        "invalid orphan bytes !\n"
    );
    read_project(&fixture.project).unwrap();
    let metadata = fixture.read("generated/update-state.json");
    configure(
        &fixture,
        &base,
        "{consumers={audit={field='count',enabled=false}}}",
    );
    assert!(codegen_project(&fixture.project).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), metadata);
    configure(&fixture, &base, "{}");
    std::fs::remove_file(fixture.project.join("consumers/audit.luau")).unwrap();
    codegen_project(&fixture.project).unwrap();
    configure(
        &fixture,
        &base,
        "{consumers={audit={field='count',enabled=false}}}",
    );
    codegen_project(&fixture.project).unwrap();
    assert!(
        fixture
            .read("consumers/audit.luau")
            .contains("NOT_IMPLEMENTED")
    );
}

#[cfg(unix)]
#[test]
fn ev_05_registered_modules_keep_the_existing_no_follow_boundary() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    configure(
        &fixture,
        &fixture.read("mock.luau"),
        "{consumers={audit={field='count',enabled=false}}}",
    );
    codegen_project(&fixture.project).unwrap();
    let source = fixture.read("consumers/audit.luau");
    let outside = fixture.base.join("outside.luau");
    std::fs::write(&outside, &source).unwrap();
    std::fs::remove_file(fixture.project.join("consumers/audit.luau")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.project.join("consumers/audit.luau")).unwrap();
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    assert!(codegen_project(&fixture.project).is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), source);
}
