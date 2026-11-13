use super::*;
use chigiri::core::Source;

fn navigation(input: InputMode) -> CheckNavigation {
    let ir = crate::ir::MockIr::parse(vec![Source::new(
        "schema.graphql",
        "type User { id: ID! custom: String } type Query { viewer: User }",
    )])
    .unwrap();
    CheckNavigation::build(&ir, input).unwrap()
}

fn report(file: &str, coordinate: &str, phase: &str) -> ReportDiagnostic {
    ReportDiagnostic {
        severity: "error".into(),
        file: Some(file.into()),
        coordinate: Some(coordinate.into()),
        member: None,
        line: Some(1),
        column: Some(1),
        embedded: None,
        phase: phase.into(),
        cause: "MOCK_CONTRACT".into(),
        expected: "declared contract".into(),
        actual: "structural type unavailable".into(),
        hint: "Inspect the declared contract.".into(),
        message: "module does not satisfy its declared contract".into(),
        references: Vec::new(),
        navigation_hook: None,
        navigation_consumer: None,
    }
}

#[test]
fn references_are_registry_proven_relevant_sorted_and_bounded() {
    let navigation = navigation(InputMode::Project);
    let mut resolver = report("resolvers/query/viewer.luau", "Query.viewer", "business");
    navigation.attach_report(&mut resolver);
    assert_eq!(
        resolver.references,
        [
            ("resolvers", "Query.viewer"),
            ("runtime", "errors"),
            ("types", "Query"),
        ]
        .map(|(topic, name)| ReportReference {
            topic: topic.into(),
            name: name.into(),
        })
    );

    let mut model = report("resolvers/user/custom.luau", "User.custom", "contract");
    navigation.attach_report(&mut model);
    assert_eq!(model.references.len(), 2);
    assert!(model.references.iter().any(|item| item.topic == "models"));

    let mut hook = report("hooks/request/before.luau", "ignored", "contract");
    hook.coordinate = None;
    hook.member = Some("request.before".into());
    hook.navigation_hook = Some("request.before".into());
    navigation.attach_report(&mut hook);
    assert_eq!(hook.references[0].topic, "hooks");
    assert!(hook.coordinate.is_none());

    let mut unknown = report("mock.luau", "arbitrary.identifier", "contract");
    navigation.attach_report(&mut unknown);
    assert!(unknown.references.is_empty());
}

#[test]
fn consumer_navigation_uses_host_provenance_without_changing_the_report_wire_shape() {
    let navigation = navigation(InputMode::Project);
    let mut diagnostic = report("consumers/audit.luau", "unknown", "contract");
    navigation.attach_report(&mut diagnostic);
    assert!(diagnostic.references.is_empty());
    diagnostic.navigation_consumer = Some("audit".into());
    navigation.attach_report(&mut diagnostic);
    assert_eq!(diagnostic.references.len(), 1);
    assert_eq!(diagnostic.references[0].topic, "consumers");
    assert_eq!(diagnostic.references[0].name, "audit");
    let wire = serde_json::to_value(&diagnostic).unwrap();
    assert!(wire.get("navigation_consumer").is_none());
}

#[test]
fn text_guidance_matches_input_mode_without_physical_paths() {
    for (mode, argument, placeholder) in [
        (InputMode::Project, "--config-dir <PROJECT>", "<PROJECT>"),
        (InputMode::Bundle, "--config <BUNDLE>", "<BUNDLE>"),
    ] {
        let navigation = navigation(mode);
        let mut diagnostic = Diagnostic::environment("safe diagnostic");
        diagnostic.set_file("resolvers/query/viewer.luau");
        diagnostic.set_coordinate("Query.viewer");
        navigation.attach_text(&mut diagnostic);
        let text = diagnostic.to_string();
        assert!(text.contains(argument));
        assert!(text.contains(placeholder));
        assert!(!text.contains("/private/"));
    }
}
