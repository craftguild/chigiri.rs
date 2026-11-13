//! Test base/user/next comparison, `.dist` ownership, stale inputs, and pre-placement plans.

use std::fs;

use super::test_support::Fixture;
use super::update_test_support::{DIST, MODULE, apply, plan, schema, state};
use super::{UpdateAction, read_project};

#[test]
fn mp_09_10_replace_propose_repeat_delete_repropose_adopt_and_independent_migration() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let old = fixture.read(MODULE);
    let next = plan(&fixture, &schema("cap")).unwrap();
    assert!(
        next.report
            .events
            .iter()
            .any(|e| e.action == UpdateAction::Replaced)
    );
    assert_ne!(next.files[MODULE], old);
    apply(&fixture, &next);
    let base = state(&fixture).resolvers.0[0].base.clone();
    let user = format!("- The user edits \n {}", fixture.read(MODULE));
    fixture.write(MODULE, &user);
    // Non-UTF-8 transitional seeds are checked by ownership, kind, and capacity rather than text parsing.
    fs::write(fixture.project.join("seeds/Item.jsonl"), [0xff, 0xfe]).unwrap();
    let next = plan(&fixture, &schema("size")).unwrap();
    assert!(!next.files.contains_key(MODULE));
    assert!(next.files.contains_key(DIST));
    apply(&fixture, &next);
    assert_eq!(fixture.read(MODULE), user);
    assert_eq!(state(&fixture).resolvers.0[0].base, base);
    assert!(read_project(&fixture.project).is_err());
    let proposal = fixture.read(DIST);
    let repeat = plan(&fixture, &schema("size")).unwrap();
    assert!(repeat.files.is_empty());
    fs::remove_file(fixture.project.join(DIST)).unwrap();
    let repeat = plan(&fixture, &schema("size")).unwrap();
    assert_eq!(repeat.files.len(), 1);
    assert_eq!(repeat.files[DIST], proposal);
    apply(&fixture, &repeat);
    fixture.write(MODULE, &proposal);
    let adopted = plan(&fixture, &schema("size")).unwrap();
    assert!(!adopted.files.contains_key(DIST));
    assert!(!adopted.files.contains_key(MODULE));
    assert!(
        adopted
            .report
            .events
            .iter()
            .any(|e| e.action == UpdateAction::Adopted)
    );
    apply(&fixture, &adopted);
    assert_eq!(
        state(&fixture).resolvers.0[0].base,
        state(&fixture).resolvers.0[0].candidate
    );
    assert_eq!(fixture.read(DIST), proposal);
    fixture.write(
        MODULE,
        &format!("-- its own custom implementation of \n {proposal}"),
    );
    fixture.write("seeds/Item.jsonl", "");
    let independent = plan(&fixture, &schema("size")).unwrap();
    assert!(independent.files.is_empty());
    assert!(
        independent
            .report
            .events
            .iter()
            .any(|e| e.action == UpdateAction::Kept)
    );
    assert!(read_project(&fixture.project).is_ok());
}

#[test]
fn mp_09_11_12_proposals_do_not_advance_base_and_retained_older_dist_is_explicit() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let pristine = fixture.read(MODULE);
    let base = state(&fixture).resolvers.0[0].base.clone();
    fixture.write(MODULE, &format!("-- handwritten behavior\n{pristine}"));
    for argument in ["cap", "size"] {
        let next = plan(&fixture, &schema(argument)).unwrap();
        assert!(next.files.contains_key(DIST));
        apply(&fixture, &next);
        assert_eq!(state(&fixture).resolvers.0[0].base, base);
    }
    let prior_dist = fixture.read(DIST);
    let reverted = plan(&fixture, &schema("limit")).unwrap();
    assert!(!reverted.files.contains_key(DIST));
    assert!(!reverted.files.contains_key(MODULE));
    assert!(
        reverted
            .report
            .events
            .iter()
            .any(|e| e.action == UpdateAction::StaleProposal && e.path == DIST)
    );
    apply(&fixture, &reverted);
    assert_eq!(fixture.read(DIST), prior_dist);
    assert!(read_project(&fixture.project).is_ok());
    assert!(plan(&fixture, &schema("limit")).unwrap().files.is_empty());
}

#[test]
fn mp_11_13_edited_unmanaged_dist_and_missing_active_are_rejected_before_any_write() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    let edited = format!("- Protected object \n {}", fixture.read(MODULE));
    fixture.write(MODULE, &edited);
    fixture.write(DIST, "unmanaged proposal");
    let before = fixture.read("generated/update-state.json");
    assert!(plan(&fixture, &schema("cap")).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), before);
    assert_eq!(fixture.read(DIST), "unmanaged proposal");
    fs::remove_file(fixture.project.join(DIST)).unwrap();
    let next = plan(&fixture, &schema("cap")).unwrap();
    apply(&fixture, &next);
    fixture.write(DIST, "edited proposal");
    let before = fixture.read("generated/update-state.json");
    assert!(plan(&fixture, &schema("size")).is_err());
    assert_eq!(fixture.read("generated/update-state.json"), before);
    assert_eq!(fixture.read(MODULE), edited);
    assert_eq!(fixture.read(DIST), "edited proposal");
    fs::remove_file(fixture.project.join(MODULE)).unwrap();
    assert!(plan(&fixture, &schema("cap")).is_err());
    assert!(!fixture.project.join(MODULE).exists());
}

#[test]
fn mp_14_update_comparison_can_preserve_non_utf8_legacy_module_without_compiling_it() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("limit"));
    fs::write(fixture.project.join(MODULE), [0xff, 0xfe]).unwrap();
    let next = plan(&fixture, &schema("cap")).unwrap();
    assert!(next.files.contains_key(DIST));
    assert!(!next.files.contains_key(MODULE));
    apply(&fixture, &next);
    assert_eq!(
        fs::read(fixture.project.join(MODULE)).unwrap(),
        [0xff, 0xfe]
    );
    assert!(read_project(&fixture.project).is_err());
}
