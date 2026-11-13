//! Test disabled-hook ownership and capacity, base/user/next comparison, and non-SDL hook/state generation.

use std::fs;

use super::digest::Digest;
use super::state::{Snapshot, State};
use super::test_support::Fixture;
use super::update_test_support::state;
use super::{ActiveSnapshot, UpdateAction, read_project, update_project};
use crate::hooks::HookSlot;

const SDL: &str = include_str!("../../fixtures/blog.graphql");

fn documents(fixture: &Fixture, state: &mut State) {
    let mut snapshot: Snapshot =
        serde_json::from_str(&fixture.read("generated/schema.snapshot.json")).unwrap();
    let (snapshot, state) = state
        .render_documents(&mut snapshot, &fixture.read("generated/types.d.luau"))
        .unwrap();
    fixture.write("generated/schema.snapshot.json", &snapshot);
    fixture.write("generated/update-state.json", &state);
}

#[test]
fn hk_01_05_06_scaffolds_have_six_owned_ready_noops_and_closed_v3_inventory() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let original = state(&fixture);
    assert_eq!(original.format, "chigiri-mock-update-state/v11");
    let saved: Snapshot =
        serde_json::from_str(&fixture.read("generated/schema.snapshot.json")).unwrap();
    assert_eq!(saved.format, "chigiri-mock-schema-snapshot/v11");
    assert_eq!(original.hooks.0.len(), 6);
    for slot in HookSlot::ALL {
        assert!(!snapshot.schema().settings().hooks.enabled(slot));
        let entry = original
            .hooks
            .0
            .iter()
            .find(|entry| entry.slot == slot)
            .unwrap();
        assert_eq!(entry.path, slot.path());
        let source = fixture.read(slot.path());
        crate::luau::format::check(&source, crate::luau::MAX_BODY).unwrap();
        assert_eq!(entry.base, Digest::bytes(source.as_bytes()));
        assert_eq!(entry.candidate, entry.base);
        assert!(entry.dist.is_none());
        assert!(source.contains("return nil"));
    }
    for variant in ["missing", "duplicate", "path", "dist"] {
        let mut changed = original.clone();
        match variant {
            "missing" => {
                changed.hooks.0.pop();
            }
            "duplicate" => changed.hooks.0[1].slot = changed.hooks.0[0].slot,
            "path" => {
                changed.hooks.0[0].path =
                    chigiri_host::PortableRelativePath::new("hooks/request/other.luau").unwrap()
            }
            _ => {
                changed.hooks.0[0].dist = Some(super::state::Dist {
                    path: chigiri_host::PortableRelativePath::new("escape.dist").unwrap(),
                    digest: Digest::bytes(b""),
                })
            }
        }
        assert!(super::inventory::validate(&changed, &saved).is_err());
    }
    for (file, current, old) in [
        (
            "generated/update-state.json",
            "chigiri-mock-update-state/v11",
            "chigiri-mock-update-state/v2",
        ),
        (
            "generated/schema.snapshot.json",
            "chigiri-mock-schema-snapshot/v11",
            "chigiri-mock-schema-snapshot/v2",
        ),
    ] {
        let source = fixture.read(file);
        let legacy = source.replace(current, old);
        fixture.write(file, &legacy);
        assert!(ActiveSnapshot::capture(&fixture.project).is_err());
        assert!(update_project(&fixture.project).is_err());
        assert_eq!(fixture.read(file), legacy);
        fixture.write(file, &source);
    }
}

#[test]
fn hk_05_disabled_hook_is_not_typechecked_but_updates_keep_bytes_and_detect_drift() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let slot = HookSlot::FieldAfter;
    fixture.write(slot.path(), "-- The user is organized.\nnot Luau !!\n");
    let before = fixture.read(slot.path());
    let report = update_project(&fixture.project).unwrap();
    assert!(
        report
            .events
            .iter()
            .any(|event| event.path == slot.path() && event.action == UpdateAction::Kept)
    );
    assert_eq!(fixture.read(slot.path()), before);
    read_project(&fixture.project).unwrap();
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    fixture.write(slot.path(), "-- Concurrent user edit.\n");
    assert!(snapshot.verify_unchanged().is_err());
    let refreshed = snapshot.refreshed().unwrap();
    refreshed.verify_unchanged().unwrap();
    fs::remove_file(fixture.project.join(slot.path())).unwrap();
    let metadata = fixture.read("generated/update-state.json");
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    assert!(update_project(&fixture.project).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), metadata);
    fixture.write(slot.path(), &" ".repeat(crate::luau::MAX_BODY + 1));
    assert!(ActiveSnapshot::capture(&fixture.project).is_err());
    assert!(update_project(&fixture.project).is_err());
}

#[test]
fn hk_05_hook_formatter_upgrade_shares_resolver_bun_and_protects_edited_dist() {
    let _case = crate::vm_gate::test_case();
    let slot = HookSlot::RequestBefore;
    let path = slot.path();
    let dist = format!("{path}.dist");
    for edited in [false, true] {
        let fixture = Fixture::new(SDL);
        let canonical = fixture.read(path);
        let old = canonical.replace("    ", "\t");
        assert_ne!(old, canonical);
        let mut ownership = state(&fixture);
        let entry = ownership
            .hooks
            .0
            .iter_mut()
            .find(|entry| entry.slot == slot)
            .unwrap();
        entry.base = Digest::bytes(old.as_bytes());
        entry.candidate = entry.base.clone();
        let base = entry.base.clone();
        documents(&fixture, &mut ownership);
        let user = if edited { format!("{old}\n") } else { old };
        fixture.write(path, &user);
        update_project(&fixture.project).unwrap();
        let after = state(&fixture);
        let entry = after
            .hooks
            .0
            .iter()
            .find(|entry| entry.slot == slot)
            .unwrap();
        if edited {
            assert_eq!(fixture.read(path), user);
            assert_eq!(fixture.read(&dist), canonical);
            assert_eq!(entry.base, base);
            assert_eq!(
                entry.dist.as_ref().unwrap().digest,
                Digest::bytes(canonical.as_bytes())
            );
        } else {
            assert_eq!(fixture.read(path), canonical);
            assert_eq!(entry.base, entry.candidate);
            assert!(!fixture.project.join(&dist).exists());
        }
        let metadata = fixture.read("generated/update-state.json");
        update_project(&fixture.project).unwrap();
        assert_eq!(fixture.read("generated/update-state.json"), metadata);
        if edited {
            fixture.write(&dist, "- The user's suggestions are edited by \n.");
            assert!(update_project(&fixture.project).is_err());
            assert_eq!(fixture.read("generated/update-state.json"), metadata);
            assert_eq!(fixture.read(path), user);
            assert_eq!(
                fixture.read(&dist),
                "- The user's suggestions are edited by \n."
            );
        }
    }
}

#[test]
fn hk_05_state_registration_and_initial_changes_advance_management_not_schema_digest() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let original = state(&fixture);
    let config = fixture.read("mock.luau");
    let with_state = config.replace(
        "state = {fields = {}, initial = {}},",
        "state = {fields = {authenticated={kind='boolean'}}, initial = {authenticated=false}},",
    );
    let mut previous = original.clone();
    for (config, changes_types) in [
        (with_state.clone(), true),
        (
            with_state.replace("authenticated=false", "authenticated=true"),
            false,
        ),
        (
            with_state.replace("request = {before = false", "request = {before = true"),
            false,
        ),
    ] {
        fixture.write("mock.luau", &config);
        assert!(ActiveSnapshot::capture(&fixture.project).is_err());
        let report = update_project(&fixture.project).unwrap();
        assert!(!report.schema_changed);
        let current = state(&fixture);
        assert_eq!(current.schema_digest, original.schema_digest);
        assert_ne!(current.generation, previous.generation);
        assert_eq!(current.types_digest != previous.types_digest, changes_types);
        ActiveSnapshot::capture(&fixture.project).unwrap();
        previous = current;
    }
    fixture.write("mock.luau", &config);
    update_project(&fixture.project).unwrap();
    let final_state = state(&fixture);
    assert_eq!(final_state.config, original.config);
    assert_eq!(final_state.schema_digest, original.schema_digest);
    for slot in HookSlot::ALL {
        assert!(fixture.project.join(slot.path()).is_file());
    }
}
