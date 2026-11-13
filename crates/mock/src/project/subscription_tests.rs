//! Verify subscription resolver placement and generated ownership without changing identities early.

use std::sync::Arc;

use chigiri::core::Source;

use super::state::{Snapshot, State};
use super::{Layout, generate_project};
use crate::ir::MockIr;

const SDL: &str = r#"
schema { query: Lookup mutation: Commands subscription: Live }
type Presence { userId: ID! online: Boolean! }
type Lookup { live: Live! }
type Commands { setPresence(userId: ID!, online: Boolean!): Presence! }
type Live { presence(teamId: ID!): Presence! }
"#;

fn ir() -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", SDL)]).unwrap())
}

#[test]
fn subscription_layout_uses_the_closed_root_role() {
    let ir = ir();
    let layout = Layout::new(&ir).unwrap();
    let entry = layout
        .resolvers
        .iter()
        .find(|entry| entry.coordinate == "Live.presence")
        .unwrap();
    assert_eq!(entry.path.as_str(), "resolvers/subscription/presence.luau");
    let subscription = ir.graph().subscription().unwrap();
    assert!(
        layout
            .seeds
            .iter()
            .all(|entry| { ir.models()[entry.model.0].ty != subscription })
    );
}

#[test]
fn subscription_generation_records_path_summary_and_exact_module() {
    let _case = crate::vm_gate::test_case();
    let project = generate_project(ir()).unwrap();
    let path = super::fixed_path("resolvers/subscription/presence.luau");
    let source = &project.files[&path];
    assert!(source.contains("ChigiriLivePresenceResolver"));
    assert!(!source.contains("ChigiriSubscriptionPresence"));
    assert!(
        project.files[&super::fixed_path("generated/types.d.luau")]
            .contains("export type ChigiriLivePresenceResolver =")
    );
    assert!(source.contains("subscribe = function("));
    assert!(source.contains("resolve = function("));
    assert!(source.contains("NOT_IMPLEMENTED"));
    let snapshot: Snapshot =
        serde_json::from_str(&project.files[&super::fixed_path("generated/schema.snapshot.json")])
            .unwrap();
    assert_eq!(snapshot.summary.subscription.as_deref(), Some("Live"));
    let state: State =
        serde_json::from_str(&project.files[&super::fixed_path("generated/update-state.json")])
            .unwrap();
    state
        .verify(
            &project.files[&super::fixed_path("generated/types.d.luau")],
            &snapshot,
            &project.files[&super::fixed_path("generated/schema.snapshot.json")],
        )
        .unwrap();
}

#[test]
fn subscription_layout_rejects_case_folded_field_collisions() {
    let ir = MockIr::parse(vec![Source::new(
        "schema/001.graphql",
        "type Query{x:Int} type Subscription{presence:Int Presence:Int}",
    )])
    .unwrap();
    assert!(Layout::new(&ir).is_err());
}
