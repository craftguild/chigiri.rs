//! Compare formatted output with pristine, empty, edited, and retained bytes before and after placement.

use std::fs;

use super::digest::Digest;
use super::state::Snapshot;
use super::test_support::Fixture;
use super::update_test_support::{DIST, MODULE, schema, state};
use super::{UpdateAction, export_project, init_project, read_project, update_project};
use crate::luau::{MAX_BODY, MAX_WRAPPER, format};

#[test]
fn fmt_01_04_05_init_formats_two_domains_and_renamed_types_before_digest() {
    let _test_case = crate::vm_gate::test_case();
    for source in [
        include_str!("../../fixtures/blog.graphql").to_owned(),
        include_str!("../../fixtures/projects.graphql").to_owned(),
        include_str!("../../fixtures/projects.graphql")
            .replace("Member", "Person")
            .replace("Query", "Lookup")
            .replace("Mutation", "Commands")
            + "\nschema { query: Lookup mutation: Commands }",
    ] {
        let fixture = Fixture::new(&source);
        let project = fixture.base.join("initial");
        init_project(&[fixture.base.join("schema.graphql")], &project).unwrap();
        let config = fs::read_to_string(project.join("mock.luau")).unwrap();
        format::check(&config, crate::limits::CONFIG_BYTES).unwrap();
        let snapshot = super::ActiveSnapshot::capture(&project).unwrap();
        let definitions = fs::read_to_string(project.join("generated/types.d.luau")).unwrap();
        assert_eq!(definitions, snapshot.contract().declarations());
        let aliases = definitions
            .strip_prefix(include_str!("../luau/host.d.luau"))
            .unwrap();
        format::check(aliases, MAX_WRAPPER).unwrap();
        let management: super::state::State = serde_json::from_str(
            &fs::read_to_string(project.join("generated/update-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            management.types_digest,
            Digest::bytes(definitions.as_bytes())
        );
        for file in snapshot.files() {
            if matches!(file.kind(), super::ActiveKind::Resolver(_)) {
                format::check(file.source(), MAX_BODY).unwrap();
                let entry = management
                    .resolvers
                    .0
                    .iter()
                    .find(|entry| entry.path == file.path())
                    .unwrap();
                assert_eq!(entry.base, Digest::bytes(file.source().as_bytes()));
            }
        }
    }
}

fn old_pristine(fixture: &Fixture, source: &str) {
    fixture.write(MODULE, source);
    let mut state = state(fixture);
    state.resolvers.0[0].base = Digest::bytes(source.as_bytes());
    state.resolvers.0[0].candidate = state.resolvers.0[0].base.clone();
    let mut snapshot: Snapshot =
        serde_json::from_str(&fixture.read("generated/schema.snapshot.json")).unwrap();
    let (snapshot, state) = state
        .render_documents(&mut snapshot, &fixture.read("generated/types.d.luau"))
        .unwrap();
    fixture.write("generated/schema.snapshot.json", &snapshot);
    fixture.write("generated/update-state.json", &state);
}

#[test]
fn fmt_06_07_08_formatter_upgrade_replaces_pristine_but_protects_whitespace_edits() {
    let _test_case = crate::vm_gate::test_case();
    for edited in [false, true] {
        let fixture = Fixture::new(&schema("limit"));
        let canonical = fixture.read(MODULE);
        let old = canonical.replace("    ", "\t");
        assert_ne!(old, canonical);
        old_pristine(&fixture, &old);
        let baseline = state(&fixture).resolvers.0[0].base.clone();
        let user = if edited { format!("{old}\n") } else { old };
        fixture.write(MODULE, &user);
        let config = fixture.read("mock.luau");
        let report = update_project(&fixture.project).unwrap();
        if edited {
            assert_eq!(fixture.read(MODULE), user);
            assert_eq!(fixture.read(DIST), canonical);
            assert_eq!(state(&fixture).resolvers.0[0].base, baseline);
            format::check(&fixture.read(DIST), MAX_BODY).unwrap();
        } else {
            assert_eq!(fixture.read(MODULE), canonical);
            assert!(
                report
                    .events
                    .iter()
                    .any(|event| event.action == UpdateAction::Replaced)
            );
            assert_eq!(
                state(&fixture).resolvers.0[0].base,
                Digest::bytes(canonical.as_bytes())
            );
        }
        let management = fixture.read("generated/update-state.json");
        update_project(&fixture.project).unwrap();
        assert_eq!(fixture.read("generated/update-state.json"), management);
        assert_eq!(fixture.read("mock.luau"), config);
    }
}

#[test]
fn fmt_09_unformatted_user_module_and_ignore_comment_survive_check_and_export() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let source = format!(
        "-- stylua: ignore\n{}\n",
        fixture.read(MODULE).replace("    ", "\t")
    );
    fixture.write(MODULE, &source);
    fixture.write(DIST, "edited dist retained");
    fixture.write("resolvers/query/orphan.luau", "orphan retained");
    let before = fixture.read("generated/update-state.json");
    let checked = read_project(&fixture.project).unwrap();
    assert_eq!(checked.scripts().scripts()[0].source(), source);
    let output = fixture.base.join("portable.toml");
    export_project(&fixture.project, &output).unwrap();
    let bundle: toml_edit::DocumentMut = fs::read_to_string(output).unwrap().parse().unwrap();
    assert_eq!(
        bundle["resolvers"]["Query.items"]["luau"].as_str(),
        Some(source.as_str())
    );
    assert_eq!(fixture.read(MODULE), source);
    assert_eq!(fixture.read("generated/update-state.json"), before);
    assert_eq!(fixture.read(DIST), "edited dist retained");
    assert_eq!(
        fixture.read("resolvers/query/orphan.luau"),
        "orphan retained"
    );
}
