//! Definition, single-event and shared bounds are independent inclusive limits.

use std::sync::Arc;

use chigiri::core::{Source, Value};

use super::{DefinitionBuilder, PublicationError, SequenceDefinition};
use crate::events::SourceLimits;
use crate::events::retention::Retention;
use crate::ir::MockIr;
use crate::output::OutputValue;

fn capture(
    limits: SourceLimits,
    retention: &Retention,
) -> Result<Arc<SequenceDefinition>, PublicationError> {
    let ir = MockIr::parse(vec![Source::new(
        "sequence.graphql",
        "schema {query: Read subscription: Signals} type Read {ping:Int} type Signals {text:String!}",
    )]).unwrap();
    let root = ir.graph().subscription().unwrap();
    let field = ir.graph().types()[root.0].fields["text"];
    let mut builder = DefinitionBuilder::new(&ir, field, 1, 1, false, limits, retention)?;
    builder.push(&ir, &OutputValue::Scalar(Value::String("captured".into())))?;
    builder.seal()
}

fn limits(queue: usize, retained: usize, definition: usize) -> SourceLimits {
    SourceLimits::new(1, queue, 1, retained, definition).unwrap()
}

#[test]
fn ev_04_09_definition_and_event_preflight_accept_equality_and_reject_one_byte_over() {
    let defaults = SourceLimits::default();
    let owner = Retention::new(defaults.max_retained_bytes());
    let definition = capture(defaults, &owner).unwrap();
    let bytes = definition.bytes();
    let event = definition.event_bytes(0).unwrap();
    assert_eq!(owner.used(), bytes);
    assert!(definition.belongs_to(&owner));
    assert!(!definition.belongs_to(&Retention::new(defaults.max_retained_bytes())));
    let shared = definition.clone();
    drop(definition);
    assert_eq!(owner.used(), bytes);
    drop(shared);
    assert_eq!(owner.used(), 0);

    let exact = limits(event, bytes + event, bytes);
    let owner = Retention::new(exact.max_retained_bytes());
    let definition = capture(exact, &owner).unwrap();
    assert_eq!(definition.bytes(), bytes);
    drop(definition);
    assert_eq!(owner.used(), 0);

    for rejected in [
        limits(event - 1, bytes + event, bytes),
        limits(event, bytes + event, bytes - 1),
        limits(event, bytes + event - 1, bytes),
    ] {
        let owner = Retention::new(rejected.max_retained_bytes());
        assert_eq!(
            capture(rejected, &owner).unwrap_err(),
            PublicationError::CaptureLimit
        );
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn ev_09_other_retained_owners_cannot_be_reclaimed_by_definition_failure() {
    let defaults = SourceLimits::default();
    let owner = Retention::new(defaults.max_retained_bytes());
    let definition = capture(defaults, &owner).unwrap();
    let bytes = definition.bytes();
    let event = definition.event_bytes(0).unwrap();
    drop(definition);

    let exact = limits(event, bytes + event, bytes);
    let owner = Retention::new(exact.max_retained_bytes());
    let unrelated = owner.reserve(event + 1, event + 1).unwrap();
    assert_eq!(
        capture(exact, &owner).unwrap_err(),
        PublicationError::CaptureLimit
    );
    assert_eq!(owner.used(), event + 1);
    drop(unrelated);
    let definition = capture(exact, &owner).unwrap();
    assert_eq!(owner.used(), bytes);
    drop(definition);
    assert_eq!(owner.used(), 0);
}
