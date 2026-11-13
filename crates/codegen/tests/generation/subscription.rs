//! Verifies generated subscription traits, adapters, assembly, and compile boundaries.

use std::collections::BTreeMap;
use std::fs;

use super::cargo;
use super::generation_fixture::{TestDirectory, fixture, read_files};

#[test]
fn subscription_group_compiles_complete_and_rejects_invalid_contracts() {
    let temporary = TestDirectory::new("subscription-group");
    temporary.copy_schema(&fixture("tests/fixtures/subscription"));
    temporary.generate();

    let generated = read_files(&temporary.join("generated"));
    assert_public_trait(&generated);
    assert_private_adapters(&generated);
    assert_assembly(&generated);

    let source = fs::read_to_string(fixture("tests/compile/subscription.rs")).unwrap();
    temporary.write_consumer_with_dependencies(
        "subscription_consumer",
        &source,
        "futures-core=\"0.3.34\"\n",
    );
    temporary.assert_consumer("test");

    assert_failure(
        &temporary,
        source.replace("resolve_tick_event", "incomplete_tick_event"),
        &["not all trait items implemented", "resolve_tick_event"],
    );
    assert_failure(
        &temporary,
        source.replace("event: &'a Self::TickEvent", "event: &'a String"),
        &["method `resolve_tick_event` has an incompatible type for trait"],
    );
    assert_failure(
        &temporary,
        source.replace(
            ") -> chigiri::FieldResult<i32> {\n        Ok(*event)\n    }",
            ") -> chigiri::FieldResult<String> {\n        Ok(event.to_string())\n    }",
        ),
        &["expected `Result<i32, FieldError>`"],
    );
    assert_failure(
        &temporary,
        source.replace(
            "type PresenceEvent = i32;",
            "type PresenceEvent = std::rc::Rc<()>;",
        ),
        &["`Rc<()>` cannot be sent between threads safely"],
    );
    assert_failure(
        &temporary,
        source.replace(
            "type PresenceEvent = i32;",
            "type PresenceEvent = std::cell::Cell<()>;",
        ),
        &["`Cell<()>` cannot be shared between threads safely"],
    );
    let non_send_source = source.replacen(
        "OwnedEvents::<\n            Self::PresenceEvent,\n        >::new()",
        "NonSendEvents::<Self::PresenceEvent>::new()",
        1,
    );
    assert_ne!(non_send_source, source);
    assert_failure(
        &temporary,
        non_send_source,
        &[
            "`Rc<()>` cannot be sent between threads safely",
            "NonSendEvents<i32>",
            "required by a bound in `SubscriptionSource::<Event, Error>::new`",
        ],
    );
    let borrowed_source = source.replacen(
        "OwnedEvents::<\n            Self::PresenceEvent,\n        >::new()",
        "BorrowedEvents::<Self::PresenceEvent> { borrowed: &args.team_id, marker: PhantomData }",
        1,
    );
    assert_ne!(borrowed_source, source);
    assert_failure(
        &temporary,
        borrowed_source,
        &["borrowed data escapes outside of method"],
    );
}

#[test]
fn subscription_synthesized_methods_reject_collisions() {
    let temporary = TestDirectory::new("subscription-method-collision");
    fs::create_dir(temporary.join("schema")).unwrap();
    fs::write(
        temporary.join("schema/schema.graphql"),
        concat!(
            "schema { query: Query subscription: Subscription }\n",
            "type Query { subscription: Subscription! }\n",
            "type Subscription { presence: Int! subscribe_presence: Int! }\n",
        ),
    )
    .unwrap();

    let output = temporary.cli("codegen");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("SF4006"), "{stderr}");
    assert!(
        stderr.contains("subscription resolver method `subscribe_presence` collides"),
        "{stderr}"
    );
}

fn assert_public_trait(generated: &BTreeMap<String, Vec<u8>>) {
    let resolver = text(generated, "src/__chigiri/resolvers/subscription.rs");
    assert!(resolver.contains("type PresenceEvent:"));
    assert!(resolver.contains("fn subscribe_presence<'a>("));
    assert!(resolver.contains("args: &'a super::SubscriptionPresenceArgs,"));
    assert!(resolver.contains("fn resolve_presence_event<'a>("));
    assert!(resolver.contains("parent: &'a T::Subscription,"));
}

fn assert_private_adapters(generated: &BTreeMap<String, Vec<u8>>) {
    let bridge = text(
        generated,
        "src/__chigiri/internal/bridge/subscription/subscription.rs",
    );
    assert_eq!(bridge.matches("erase_subscription_source").count(), 2);
    assert_eq!(bridge.matches("map_err").count(), 4);
    assert!(bridge.contains("downcast_ref::<super::SubscriptionPresenceArgs>()"));
    assert!(bridge.contains("downcast_ref::<<R as super::SubscriptionResolver"));
}

fn assert_assembly(generated: &BTreeMap<String, Vec<u8>>) {
    let schema_types = text(generated, "src/__chigiri/schema_types.rs");
    assert!(
        schema_types.find("type Subscription:").unwrap()
            < schema_types.find("type Presence:").unwrap()
    );
    let assembly = text(generated, "src/__chigiri/assembly.rs");
    assert_eq!(assembly.matches("pub(super) subscription:").count(), 2);
    assert!(assembly.contains("SubscriptionResolverGroupDispatch<C>"));
    assert!(assembly.find("with_query").unwrap() < assembly.find("with_subscription").unwrap());
    assert!(assembly.find("with_subscription").unwrap() < assembly.find("with_presence").unwrap());
    let dispatch = text(generated, "src/__chigiri/internal/dispatch/subscription.rs");
    assert!(dispatch.contains("SubscriptionResolverGroupDispatch<C>"));
    let top = text(generated, "src/__chigiri/internal/bridge/dispatch.rs");
    assert!(top.contains("SubscriptionResolverDispatch<C>"));
    assert!(top.contains("::std::option::Option::Some(\"Subscription\")"));
}

fn assert_failure(temporary: &TestDirectory, source: String, expected: &[&str]) {
    temporary.write_consumer_with_dependencies(
        "subscription_consumer",
        &source,
        "futures-core=\"0.3.34\"\n",
    );
    let manifest = temporary.join("consumer/Cargo.toml");
    let output = cargo::run(&mut cargo::command(&manifest, "check"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "stderr must contain `{fragment}`:\n{stderr}"
        );
    }
}

fn text<'a>(generated: &'a BTreeMap<String, Vec<u8>>, path: &str) -> &'a str {
    std::str::from_utf8(&generated[path]).unwrap()
}
