//! Verifies that typestate, final snapshots, and direct/HTTP paths share one UI capability graph.

use super::support::{NeverDeadline, block_on, execute, resolvers};
use chigiri::spi::{ApplicationMetadata, BufferedBody, RequestHeader};
use chigiri::{ConfigurationOrigin, Endpoint};

const INTROSPECTION: &str = "{ __schema { queryType { name } } }";

#[test]
fn dx35_15_typestate_and_final_graph_preserve_independent_introspection_in_both_orders() {
    for steps in [
        vec![],
        vec![false],
        vec![true],
        vec![true, false],
        vec![false, true],
        vec![true, true, false],
    ] {
        for split in 0..=steps.len() {
            for independent in [0, 1, 2] {
                let mut missing = Endpoint::builder(resolvers());
                if independent == 1 {
                    missing = missing.inject(|parts| {
                        parts
                            .execution()
                            .dangerously_enable_introspection("independent-before");
                    });
                }
                for enabled in &steps[..split] {
                    missing = missing.graphiql(*enabled);
                }
                let mut ready = missing.without_context();
                for enabled in &steps[split..] {
                    ready = ready.graphiql(*enabled);
                }
                if independent == 2 {
                    ready = ready.inject(|parts| {
                        parts
                            .execution()
                            .dangerously_enable_introspection("independent-after");
                    });
                }
                let endpoint = ready.build().unwrap();
                let graph = steps.last().copied().unwrap_or(false);
                assert_eq!(endpoint.graphiql().is_some(), graph);
                assert_eq!(endpoint.clone().graphiql().is_some(), graph);
                let visible = graph || independent != 0;
                let configuration = endpoint.configuration();
                assert!(!*configuration.graphiql_enabled().baseline());
                assert_eq!(*configuration.graphiql_enabled().effective(), graph);
                assert_eq!(
                    configuration.graphiql_enabled().origin(),
                    if steps.is_empty() {
                        ConfigurationOrigin::Baseline
                    } else {
                        ConfigurationOrigin::User
                    }
                );
                assert_eq!(*configuration.introspection_enabled().effective(), visible);
                assert_eq!(
                    configuration.introspection_enabled().origin(),
                    if visible {
                        ConfigurationOrigin::User
                    } else {
                        ConfigurationOrigin::Baseline
                    }
                );
                let reasons = configuration.safety_overrides();
                assert_eq!(
                    reasons.len(),
                    usize::from(graph) + usize::from(independent != 0)
                );
                assert_eq!(
                    reasons
                        .iter()
                        .filter(|item| item.protection() == "graphiql-schema-introspection")
                        .count(),
                    usize::from(graph)
                );
                if independent != 0 {
                    assert_eq!(
                        reasons
                            .iter()
                            .find(|item| item.protection() == "schema-introspection")
                            .unwrap()
                            .reason(),
                        if independent == 1 {
                            "independent-before"
                        } else {
                            "independent-after"
                        }
                    );
                }
                assert_eq!(execute(&endpoint, INTROSPECTION).errors.is_empty(), visible);
                assert!(execute(&endpoint, "{ greeting }").errors.is_empty());
                assert_eq!(endpoint.clone().configuration(), configuration);
                assert_neutral(&endpoint, visible);
            }
        }
    }
}

fn assert_neutral(endpoint: &Endpoint<()>, visible: bool) {
    let metadata = ApplicationMetadata::default();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let body = serde_json::to_vec(&serde_json::json!({"query": INTROSPECTION})).unwrap();
    let response = block_on(endpoint.handle(request, BufferedBody::new(body), &NeverDeadline));
    // Introspection denial is an existing runtime execution policy error, and HTTP status is 200 as usual.
    assert_eq!(response.status, 200);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value.get("errors").is_none(), visible);
}

#[test]
fn dx35_15_repeated_explicit_reason_is_last_value_and_graph_cancellation_does_not_erase_it() {
    let endpoint = Endpoint::builder(resolvers())
        .graphiql(true)
        .inject(|parts| {
            parts
                .execution()
                .dangerously_enable_introspection("old-reason");
        })
        .without_context()
        .inject(|parts| {
            parts
                .execution()
                .dangerously_enable_introspection("final-reason");
        })
        .graphiql(false)
        .build()
        .unwrap();
    let reasons = endpoint.configuration().safety_overrides();
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].reason(), "final-reason");
    assert!(execute(&endpoint, INTROSPECTION).errors.is_empty());
}

#[test]
fn dx35_15_built_schema_uses_the_same_final_publication_setting() {
    for enabled in [false, true] {
        let endpoint = Endpoint::builder(super::support::schema())
            .graphiql(true)
            .without_context()
            .graphiql(enabled)
            .build()
            .unwrap();
        assert_eq!(endpoint.graphiql().is_some(), enabled);
        assert_eq!(execute(&endpoint, INTROSPECTION).errors.is_empty(), enabled);
        assert_neutral(&endpoint, enabled);
    }
}
