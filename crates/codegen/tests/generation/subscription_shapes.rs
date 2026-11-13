//! Compiles subscription event output shapes at the generated trait boundary.

use std::fs;

use super::cargo;
use super::generation_fixture::{TestDirectory, fixture};

#[test]
fn subscription_output_shapes_compile_and_reject_each_mismatch() {
    let temporary = TestDirectory::new("subscription-output-shapes");
    temporary.copy_schema(&fixture("tests/fixtures/subscription-shapes"));
    temporary.generate();

    let source = fs::read_to_string(fixture("tests/compile/subscription_shapes.rs")).unwrap();
    temporary.write_consumer_with_dependencies(
        "subscription_shapes_consumer",
        &source,
        "futures-core=\"0.3.34\"\n",
    );
    temporary.assert_consumer("check");

    assert_failure(
        &temporary,
        replace_once(
            replace_once(
                &source,
                "Result<Option<Vec<Option<Presence>>>, AppError>",
                "Result<Vec<Option<Presence>>, AppError>",
            ),
            "Ok(Some(vec![Some(presence())]))",
            "Ok(vec![Some(presence())])",
        ),
        &[
            "resolve_maybe_presences_event",
            "Option<Vec<Option<Presence>>>",
            "Vec<Option<Presence>>",
        ],
    );
    assert_failure(
        &temporary,
        replace_once(
            replace_once(
                &source,
                "Result<Option<Vec<Option<Presence>>>, AppError>",
                "Result<Option<Vec<Presence>>, AppError>",
            ),
            "Ok(Some(vec![Some(presence())]))",
            "Ok(Some(vec![presence()]))",
        ),
        &[
            "resolve_maybe_presences_event",
            "Vec<Option<Presence>>",
            "Vec<Presence>",
        ],
    );
    assert_failure(
        &temporary,
        replace_once(
            replace_once(
                &source,
                "Result<generated::EventNode<Types>, AppError>",
                "Result<Presence, AppError>",
            ),
            "Ok(generated::EventNode::Presence(presence()))",
            "Ok(presence())",
        ),
        &["resolve_node_event", "EventNode<Types>", "Presence"],
    );
    assert_failure(
        &temporary,
        replace_once(
            replace_once(
                &source,
                "Result<generated::EventResult<Types>, AppError>",
                "Result<Alert, AppError>",
            ),
            "Ok(generated::EventResult::Alert(alert()))",
            "Ok(alert())",
        ),
        &["resolve_result_event", "EventResult<Types>", "Alert"],
    );
    assert_failure(
        &temporary,
        replace_once(
            &source,
            "Result<Option<Vec<Option<Presence>>>, AppError>",
            "Result<Option<Vec<Option<Presence>>>, chigiri::FieldError>",
        ),
        &["resolve_maybe_presences_event", "AppError", "FieldError"],
    );
    assert_failure(
        &temporary,
        replace_subscription_error(&source),
        &[
            "SubscriptionResolver<(), Types, AppError>",
            "with_subscription",
            "OtherError",
        ],
    );
}

fn replace_once(source: impl AsRef<str>, from: &str, to: &str) -> String {
    let source = source.as_ref();
    assert_eq!(source.matches(from).count(), 1, "unique source mutation");
    let output = source.replacen(from, to, 1);
    assert_ne!(output, source);
    output
}

fn replace_subscription_error(source: &str) -> String {
    const START: &str = "// BEGIN SUBSCRIPTION GROUP";
    const END: &str = "// END SUBSCRIPTION GROUP";
    let start = source.find(START).expect("subscription group start");
    let end = source.find(END).expect("subscription group end");
    let group = &source[start..end];
    assert!(group.matches("AppError").count() > 1);
    let changed = group.replace("AppError", "OtherError");
    assert_ne!(changed, group);
    format!("{}{}{}", &source[..start], changed, &source[end..])
}

fn assert_failure(temporary: &TestDirectory, source: String, expected: &[&str]) {
    temporary.write_consumer_with_dependencies(
        "subscription_shapes_consumer",
        &source,
        "futures-core=\"0.3.34\"\n",
    );
    let manifest = temporary.join("consumer/Cargo.toml");
    let output = cargo::run(&mut cargo::command(&manifest, "check"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "consumer unexpectedly compiled");
    assert!(!stderr.contains("could not execute process"), "{stderr}");
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "stderr must contain `{fragment}`:\n{stderr}"
        );
    }
}
