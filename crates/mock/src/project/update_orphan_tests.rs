//! Test deleted and reintroduced orphans, ownership collisions, schema changes, snapshots, and management files.

use std::fs;

use super::digest::Digest;
use super::state::{Resolver, Snapshot};
use super::test_support::Fixture;
use super::update_test_support::{DIST, MODULE, apply, plan, schema, state};
use super::{UpdateAction, read_project};

const REMOVED: &str = "type Query { marker:Boolean! }";

#[test]
fn mp_12_14_removed_resolver_seed_are_inactive_orphans_and_reintroduction_requires_manual_cleanup()
{
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let old = fixture.read(MODULE);
    fs::write(fixture.project.join("seeds/Item.jsonl"), [0xff]).unwrap();
    let removed = plan(&fixture, REMOVED).unwrap();
    assert!(!removed.files.contains_key(MODULE));
    assert!(!removed.files.contains_key("seeds/Item.jsonl"));
    apply(&fixture, &removed);
    assert_eq!(fixture.read(MODULE), old);
    assert_eq!(
        fs::read(fixture.project.join("seeds/Item.jsonl")).unwrap(),
        [0xff]
    );
    assert!(
        state(&fixture)
            .resolvers
            .0
            .iter()
            .any(|e| e.path == MODULE && !e.active)
    );
    assert!(state(&fixture).seeds.0.iter().all(|e| !e.active));
    let schema_input = super::read_project_schema(&fixture.project).unwrap();
    let module =
        crate::luau::render_module(schema_input.ir(), crate::ir::ScriptId(0), "return true")
            .unwrap();
    fixture.write("resolvers/query/marker.luau", &module);
    assert!(read_project(&fixture.project).is_ok());
    assert!(plan(&fixture, &schema("limit")).is_err());
    fs::remove_file(fixture.project.join(MODULE)).unwrap();
    fs::remove_file(fixture.project.join("seeds/Item.jsonl")).unwrap();
    let reintroduced = plan(&fixture, &schema("limit")).unwrap();
    assert!(reintroduced.files.contains_key(MODULE));
    assert_eq!(reintroduced.files["seeds/Item.jsonl"], "");
    assert_eq!(
        reintroduced
            .report
            .events
            .iter()
            .filter(|e| e.action == UpdateAction::OrphanRemoved)
            .count(),
        2
    );
    apply(&fixture, &reintroduced);
    assert!(read_project(&fixture.project).is_ok());
    assert!(fixture.project.join("resolvers/query/marker.luau").exists());
}

#[test]
fn mp_11_12_orphan_dist_is_never_deleted_or_silently_claimed_by_a_reintroduced_path() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    fixture.write(
        MODULE,
        &format!("- The user edits \n {}", fixture.read(MODULE)),
    );
    apply(&fixture, &plan(&fixture, &schema("cap")).unwrap());
    let dist = fixture.read(DIST);
    apply(&fixture, &plan(&fixture, REMOVED).unwrap());
    fs::remove_file(fixture.project.join(MODULE)).unwrap();
    fs::remove_file(fixture.project.join("seeds/Item.jsonl")).unwrap();
    assert_eq!(plan(&fixture, &schema("cap")).err().unwrap().file, MODULE);
    assert_eq!(fixture.read(DIST), dist);
    fs::remove_file(fixture.project.join(DIST)).unwrap();
    assert!(plan(&fixture, &schema("cap")).is_ok());
}

#[test]
fn mp_12_13_new_coordinate_seed_and_case_collisions_are_preflight_failures() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let before = fixture.read("generated/update-state.json");
    fixture.write("resolvers/query/extra.luau", "existing user file");
    let next = format!("{} extend type Query {{ extra:Int }}", schema("cap"));
    assert!(plan(&fixture, &next).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), before);
    fixture.write("seeds/Other.jsonl", "existing seed");
    let next = format!("{} type Other {{id:ID!}}", schema("limit"));
    assert!(plan(&fixture, &next).is_err());
    assert_eq!(fixture.read("seeds/Other.jsonl"), "existing seed");
    let lower = schema("limit").replace("Item", "item");
    assert!(plan(&fixture, &lower).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), before);
    let renamed_root =
        schema("limit").replace("type Query", "type Lookup") + " schema {query:Lookup}";
    assert_eq!(plan(&fixture, &renamed_root).err().unwrap().file, MODULE);
}

#[test]
fn mp_12_schema_summary_reports_model_key_field_argument_constraint_and_relation_changes() {
    let _test_case = crate::vm_gate::test_case();
    let old = "type Item { id:ID! code:String! title:String! @constraint(maxLength:4) peerId:ID peer:Peer @relation(fields:[\"peerId\"], references:[\"id\"]) } type Peer { id:ID! } type Query { items(limit:Int! = 20):[Item!]! }".to_owned();
    let fixture = Fixture::new(&old);
    let next = old
        .replace("maxLength:4", "maxLength:8")
        .replace("peerId", "peerRef")
        .replace("Peer", "Target")
        .replace("items(limit:", "items(cap:")
        .replace("limit:\"limit\"", "limit:\"cap\"");
    let plan = plan(&fixture, &next).unwrap();
    for (element, action) in [
        ("constraint/Item.title", "changed"),
        ("field/Item.title", "changed"),
        ("relation/Item.peer", "changed"),
        ("model/Peer", "removed"),
        ("model/Target", "added"),
        ("argument/Query.items/limit", "removed"),
        ("argument/Query.items/cap", "added"),
    ] {
        assert!(
            plan.report
                .schema_changes
                .iter()
                .any(|e| e.element == element && e.action == action),
            "{element}"
        );
    }
}

#[test]
fn mp_13_15_start_observations_reject_seed_module_metadata_or_absent_destination_changes() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    for path in [MODULE, "seeds/Item.jsonl", "generated/update-state.json"] {
        let before = fixture.read(path);
        let next = plan(&fixture, &schema("cap")).unwrap();
        next.inputs.verify().unwrap();
        fixture.write(path, "concurrent edit");
        assert!(next.inputs.verify().is_err());
        fixture.write(path, &before);
    }
    fixture.write(MODULE, &format!("-- edited\n{}", fixture.read(MODULE)));
    let next = plan(&fixture, &schema("cap")).unwrap();
    fixture.write(DIST, "late user proposal");
    assert!(next.inputs.verify().is_err());
    assert_eq!(fixture.read(DIST), "late user proposal");
}

#[test]
fn mp_31_combined_managed_file_limit_includes_orphans_and_fixed_files_exactly() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let original = state(&fixture);
    let snapshot: Snapshot =
        serde_json::from_str(&fixture.read("generated/schema.snapshot.json")).unwrap();
    for count in [1012, 1013] {
        let mut state = original.clone();
        let mut snapshot = snapshot.clone();
        for index in 0..count {
            state.resolvers.0.push(Resolver {
                coordinate: format!("Query.old{index}"),
                path: chigiri_host::PortableRelativePath::new(&format!(
                    "resolvers/query/old{index}.luau"
                ))
                .unwrap(),
                base: Digest::bytes(b"old"),
                candidate: Digest::bytes(b"old"),
                dist: None,
                active: false,
            });
        }
        assert_eq!(state.file_count(), count + 12);
        assert_eq!(
            state
                .render_documents(&mut snapshot, &fixture.read("generated/types.d.luau"))
                .is_ok(),
            count == 1012
        );
    }
}
