//! Bound destination metadata while capturing one payload per type publication.

use super::super::batch::BatchBuilder;
use super::super::meter::{ENTRY_BYTES, LENGTH_BYTES};
use super::super::topics::TopicIndex;
use super::super::{PublicationError, PublicationLimits};
use super::support::{fixture, object_output};

#[test]
fn fanout_counts_once_and_prepays_every_destination_before_capture() {
    let fixture = fixture();
    let output = object_output(&fixture, fixture.dto, false);
    let fields = [fixture.dto, fixture.stored];
    let mut single = BatchBuilder::new(PublicationLimits::default()).unwrap();
    single
        .push(&fixture.ir, fields[0], "same", &output)
        .unwrap();
    let expected = single.bytes() + LENGTH_BYTES + ENTRY_BYTES + "Subscription.stored".len();
    let mut exact = BatchBuilder::new(PublicationLimits::new(1, expected).unwrap()).unwrap();
    exact
        .push_matching(&fixture.ir, &fields, "same", &output)
        .unwrap();
    assert_eq!((exact.len(), exact.bytes()), (1, expected));
    assert_eq!(
        exact.push_matching(&fixture.ir, &fields, "same", &output),
        Err(PublicationError::PublicationCount)
    );
    let batch = exact.seal().unwrap();
    assert_eq!(batch.publications().len(), 1);
    assert_eq!(
        batch.publications()[0].coordinates().collect::<Vec<_>>(),
        ["Subscription.dto", "Subscription.stored"]
    );
    let mut short = BatchBuilder::new(PublicationLimits::new(1, expected - 1).unwrap()).unwrap();
    assert_eq!(
        short.push_matching(&fixture.ir, &fields, "same", &output),
        Err(PublicationError::CaptureLimit)
    );
    assert_eq!((short.len(), short.bytes()), (0, 0));
    for invalid in [
        vec![],
        vec![fields[0], fields[0]],
        vec![fields[0], fixture.event],
    ] {
        let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
        assert_eq!(
            builder.push_matching(&fixture.ir, &invalid, "same", &output),
            Err(PublicationError::InvalidCoordinate)
        );
        assert_eq!((builder.len(), builder.bytes()), (0, 0));
    }
}

#[test]
fn a_lost_type_publication_marks_only_compatible_destinations_at_that_topic() {
    let fixture = fixture();
    let mut index = TopicIndex::new(3).unwrap();
    let mut registrations = Vec::new();
    for field in [fixture.event, fixture.dto, fixture.stored] {
        let prepared = index.prepare_binding(&fixture.ir, field, "same").unwrap();
        let outcome = index.register(prepared).ok().unwrap();
        registrations.push(outcome.binding);
        drop(outcome.retired_topic_candidate);
    }
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push_matching(
            &fixture.ir,
            &[fixture.dto, fixture.stored],
            "same",
            &object_output(&fixture, fixture.dto, false),
        )
        .unwrap();
    let sequence = index.commit_sequence().unwrap();
    index
        .raise_loss(&sequence, &builder.seal().unwrap())
        .unwrap();
    for (coordinate, lost) in [
        ("Subscription.event", false),
        ("Subscription.dto", true),
        ("Subscription.stored", true),
    ] {
        assert_eq!(
            index
                .next_binding("same", coordinate, 0, sequence.value())
                .unwrap()
                .lost,
            lost
        );
    }
    drop(registrations);
}
