//! Verifies that text references preserve their input authority at every schema-derived surface.

use super::{Format, InputKind, ReferenceDocument, Selection, Topic};
use crate::hooks::{HookSettings, StateContract};
use crate::ir::MockIr;
use chigiri::core::Source;

fn selection(topic: Option<Topic>, name: Option<&str>) -> Selection {
    Selection::new(topic, name.map(str::to_owned)).unwrap()
}

fn text(document: &ReferenceDocument, topic: Option<Topic>, name: Option<&str>) -> String {
    String::from_utf8(
        document
            .render_selection(Format::Text, &selection(topic, name))
            .unwrap(),
    )
    .unwrap()
}

fn assert_surfaces(
    document: &ReferenceDocument,
    boundary: &str,
    input_option: &str,
    forbidden_options: &[&str],
) {
    let surfaces = [
        text(document, None, None),
        text(document, Some(Topic::Models), None),
        text(document, Some(Topic::Directives), Some("policy")),
        text(document, Some(Topic::Methods), Some("find")),
        text(document, Some(Topic::Hooks), Some("request.before")),
        text(document, Some(Topic::Models), Some("Account")),
        text(document, Some(Topic::Types), Some("Account")),
        text(document, Some(Topic::Resolvers), Some("Query.account")),
        text(document, Some(Topic::Runtime), Some("timeleap")),
        text(document, Some(Topic::All), None),
    ];
    for output in &surfaces {
        assert!(output.contains(boundary), "{output}");
        assert!(output.contains("readiness") && output.contains("not checked"));
    }
    for output in &surfaces[2..9] {
        assert!(output.contains(input_option), "{output}");
        for forbidden in forbidden_options {
            assert!(!output.contains(forbidden), "{output}");
        }
    }
}

#[test]
fn r52_03_text_surfaces_distinguish_project_schema_and_bundle_authority() {
    let ir = MockIr::parse(vec![Source::new(
        "logical/schema.graphql",
        concat!(
            "\"Adopted policy description.\" directive @policy on FIELD_DEFINITION ",
            "type Account { id: ID! } type Query { account: Account @policy }",
        ),
    )])
    .unwrap();
    let state = StateContract::default();
    let mut settings = HookSettings::default();
    settings.request.before = true;
    let project = ReferenceDocument::from_configuration(
        InputKind::Project,
        &ir,
        &state,
        &settings,
        &crate::events::EventSettings::default(),
    )
    .unwrap();
    assert_surfaces(
        &project,
        "Project input uses adopted public SDL plus current project-local storage",
        "--config-dir PROJECT",
        &["--schema SDL", "--config BUNDLE"],
    );
    assert!(
        text(&project, Some(Topic::Hooks), Some("request.before")).contains("enabled=Some(true)")
    );

    let schema = ReferenceDocument::from_input(InputKind::Schema, Some(&ir)).unwrap();
    assert_surfaces(
        &schema,
        "Schema input uses explicitly supplied live SDL and does not read or update a project",
        "--schema SDL",
        &["--config-dir PROJECT", "--config BUNDLE"],
    );

    let bundle = ReferenceDocument::from_configuration(
        InputKind::Bundle,
        &ir,
        &state,
        &settings,
        &crate::events::EventSettings::default(),
    )
    .unwrap();
    assert_surfaces(
        &bundle,
        "Bundle input uses SDL embedded in the portable bundle",
        "--config BUNDLE",
        &["--schema SDL", "--config-dir PROJECT"],
    );
}
