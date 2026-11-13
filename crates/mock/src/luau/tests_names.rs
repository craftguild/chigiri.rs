//! Verify public-name locations and links to declarations used by the type checker.

use std::sync::Arc;

use chigiri::core::Source;

use super::{Contract, names, static_candidates};
use crate::ir::{MockIr, ScriptId};

#[test]
fn ev_05_configuration_aliases_cannot_collide_with_custom_subscription_roots() {
    for root in [
        "EventLimits",
        "ConsumerRegistration",
        "EventSettings",
        "ClockConfig",
    ] {
        let sdl = format!(
            "schema {{ query: Query subscription: {root} }} type Query {{ count: Int! }} type {root} {{ changed: Int! }}"
        );
        let ir = MockIr::parse(vec![Source::new("schema.graphql", sdl)]).unwrap();
        assert!(names::validate(&ir).is_err(), "{root}");
    }
}

#[test]
fn mp_02_public_types_and_candidates_are_stable_across_unrelated_ir_changes() {
    let base = include_str!("../../fixtures/blog.graphql");
    let expanded = format!("type AAA {{ z: String! }}\n{base}\nextend type Query {{ aaa: AAA! }}");
    let candidates = |source| {
        let ir = Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", source)]).unwrap());
        let contract = Contract::new(ir.clone()).unwrap();
        assert!(contract.declarations().contains("type ChigiriUserRecord ="));
        assert!(
            contract
                .declarations()
                .contains("type ChigiriQueryGetUsersArgs =")
        );
        // All declarations of the configuration authority, including multiple alias export indices and fixed constants, are consistent.
        let config = super::format::source(
            &crate::project::CONFIG_DECLARATIONS.replace("type Chigiri", "export type Chigiri"),
            super::MAX_WRAPPER,
        )
        .unwrap();
        assert!(contract.declarations().contains(&config));
        static_candidates(&ir)
            .unwrap()
            .into_iter()
            .map(|candidate| {
                (
                    names::resolver(&ir, candidate.script, "Resolver"),
                    candidate.source,
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let original = candidates(base);
    let changed = candidates(&expanded);
    for (name, body) in &original {
        assert_eq!(&changed[name], body);
    }
    let renamed = base
        .replace("Query", "Lookup")
        .replace("Mutation", "Commands")
        + "\nschema { query: Lookup mutation: Commands }";
    assert_eq!(original, candidates(&renamed));
}

#[test]
fn mp_02_public_type_names_use_source_names_without_suffixes() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "schema/001.graphql",
            "type Query { end: Int! }",
        )])
        .unwrap(),
    );
    assert!(Contract::new(ir.clone()).is_ok());
    assert_eq!(
        names::resolver(&ir, ScriptId(0), "Resolver"),
        "ChigiriQueryEndResolver"
    );
}
