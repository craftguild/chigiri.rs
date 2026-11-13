//! Structurally compares the synthetic legacy corpus with the Chigiri Axum response.

#[path = "differential/policy.rs"]
mod policy;
#[path = "differential/root_markers.rs"]
mod root_markers;
#[path = "differential/support.rs"]
mod support;

use chigiri_axum_adoption::differential::{
    ComparisonCase, ObservedResponse, compare_corpus, parse_allowlist,
};

#[tokio::test]
async fn diff_01_synthetic_corpus_has_no_unallowlisted_external_structural_differences() {
    let cases = support::load_cases(include_str!("fixtures/cases.json"));
    let allowlist = parse_allowlist(include_str!("fixtures/allowlist.json")).unwrap();
    let actual = support::execute_new(&cases).await;
    let comparisons = cases
        .iter()
        .zip(&actual)
        .map(|(case, new)| ComparisonCase {
            case_id: &case.id,
            old: &case.legacy,
            new,
            header_names: &case.compare_headers,
            volatile_extension_pointers: &case.volatile_extension_pointers,
        })
        .collect::<Vec<_>>();

    compare_corpus(&comparisons, &allowlist).unwrap();
}

pub(crate) fn response(body: serde_json::Value) -> ObservedResponse {
    ObservedResponse {
        status: 200,
        media_type: "application/graphql-response+json; charset=utf-8".to_owned(),
        headers: Default::default(),
        body,
    }
}
