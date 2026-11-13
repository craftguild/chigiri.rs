//! Build HTTP fixtures without an embedded SDL module or fallback reader path.

use chigiri::core::{SchemaDigest, Source};
use chigiri_mock::ir::{MockIr, ScriptId};
use chigiri_mock::luau::render_module;
use toml_edit::DocumentMut;

pub fn module(document: &DocumentMut, coordinate: &str, body: &str) -> String {
    const PLACEHOLDER: &str = "error(\"CHIGIRI_TEST_BODY_PLACEHOLDER\")";
    let schema = &document["schema"];
    let sources = schema["sources"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .map(|source| {
            Source::new(
                source["name"].as_str().unwrap(),
                source["text"].as_str().unwrap(),
            )
        })
        .collect();
    let ir = MockIr::from_snapshot(
        sources,
        SchemaDigest::from_hex(schema["digest"].as_str().unwrap()).unwrap(),
        schema["vocabulary"].as_str().unwrap(),
    )
    .unwrap();
    let id = ir
        .scripts()
        .iter()
        .position(|plan| {
            let field = &ir.graph().fields()[plan.field.0];
            format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name) == coordinate
        })
        .unwrap();
    // Build a top-level rejection fixture by replacing only the smaller module's core body.
    // This helper is test-only; renderer capacity is covered separately.
    render_module(&ir, ScriptId(id), PLACEHOLDER)
        .unwrap()
        .replace(PLACEHOLDER, body)
}
