//! Reject mixed identities, forged metadata, ordinary reads, and collisions without writes.

use std::fs;

use super::support::LegacyFixture;
use crate::project::filesystem::TRANSACTION;
use crate::project::{
    SchemaSyncStatus, codegen_project, compare_public_schema, export_project, read_project,
    read_project_schema, update_project,
};

fn rejects(edit: impl FnOnce(&LegacyFixture)) {
    let fixture = LegacyFixture::new();
    edit(&fixture);
    let before = fixture.tree();
    assert!(update_project(&fixture.project).is_err());
    assert_eq!(fixture.tree(), before);
}

#[test]
fn ev_12_every_reader_rejects_the_exact_legacy_generation_without_writes() {
    let _case = crate::vm_gate::test_case();
    let fixture = LegacyFixture::new();
    let output = fixture.base.join("portable.toml");
    let before = fixture.tree();
    assert!(update_project(&fixture.project).is_err());
    assert!(read_project(&fixture.project).is_err());
    assert!(read_project_schema(&fixture.project).is_err());
    assert!(codegen_project(&fixture.project).is_err());
    assert!(export_project(&fixture.project, &output).is_err());
    assert!(!output.exists());
    assert_eq!(
        compare_public_schema(&fixture.project).status,
        SchemaSyncStatus::Unavailable
    );
    assert_eq!(fixture.tree(), before);
}

#[test]
fn ev_12_legacy_update_never_enters_the_placement_transaction() {
    let _case = crate::vm_gate::test_case();
    let fixture = LegacyFixture::new();
    let before = fixture.tree();
    let mut placements = 0;
    let error = crate::project::update::update_with(&fixture.project, |_| {
        placements += 1;
        Ok(())
    })
    .err()
    .unwrap();
    assert_eq!(placements, 0);
    assert!(!error.committed);
    assert!(error.recovery.is_empty());
    assert_eq!(fixture.tree(), before);
}

#[test]
fn mixed_current_and_legacy_identity_pairs_never_fall_back() {
    let _case = crate::vm_gate::test_case();
    rejects(|fixture| {
        let config = fixture.read("mock.luau");
        fixture.write(
            "mock.luau",
            &config.replace("chigiri-mock-project/v8", "chigiri-mock-project/v10"),
        );
    });
    for (path, old, current) in [
        (
            "generated/update-state.json",
            "chigiri-mock-update-state/v9",
            "chigiri-mock-update-state/v11",
        ),
        (
            "generated/schema.snapshot.json",
            "chigiri-mock-schema-snapshot/v9",
            "chigiri-mock-schema-snapshot/v11",
        ),
    ] {
        rejects(|fixture| {
            let document = fixture.read(path);
            fixture.write(path, &document.replace(old, current));
        });
    }
    rejects(|fixture| {
        let state = fixture.read("generated/update-state.json");
        fixture.write(
            "generated/update-state.json",
            &state.replacen(
                "chigiri-mock-capabilities/v2",
                "chigiri-mock-capabilities/v4",
                1,
            ),
        );
    });
}

#[test]
fn forged_ownership_inventory_and_destinations_are_rejected_without_writes() {
    let _case = crate::vm_gate::test_case();
    rejects(|fixture| {
        let state = fixture.read("generated/update-state.json");
        fixture.write(
            "generated/update-state.json",
            &state.replacen("335d4627", "435d4627", 1),
        );
    });
    rejects(|fixture| {
        let snapshot = fixture.read("generated/schema.snapshot.json");
        fixture.write(
            "generated/schema.snapshot.json",
            &snapshot.replace(
                "\"mutation\": \"Mutation\",",
                "\"mutation\": \"Mutation\",\n    \"subscription\": \"Subscription\",",
            ),
        );
    });
    rejects(|fixture| {
        let state = fixture.read("generated/update-state.json");
        fixture.write(
            "generated/update-state.json",
            &state.replacen(
                "\"format\": \"chigiri-mock-update-state/v9\",",
                "\"format\": \"chigiri-mock-update-state/v9\",\n  \"unknown\": true,",
                1,
            ),
        );
    });
    rejects(|fixture| {
        let snapshot = fixture.read("generated/schema.snapshot.json");
        fixture.write(
            "generated/schema.snapshot.json",
            &snapshot.replacen(
                "\"format\": \"chigiri-mock-schema-snapshot/v9\",",
                "\"format\": \"chigiri-mock-schema-snapshot/v9\",\n  \"unknown\": true,",
                1,
            ),
        );
    });
    rejects(|fixture| {
        let types = fixture.read("generated/types.d.luau");
        fixture.write("generated/types.d.luau", &(types + "-- forged\n"));
    });
    rejects(|fixture| {
        fixture.write(
            "resolvers/subscription/todoChanged.luau",
            "return { subscribe = function() error('occupied') end }\n",
        );
    });
    rejects(|fixture| {
        fixture.write("resolvers/query/todos.luau.dist", "occupied proposal\n");
    });
    rejects(|fixture| {
        fs::create_dir(fixture.project.join(TRANSACTION)).unwrap();
    });
}
