//! Explain bounded event authoring and include exact adopted destination signatures.

use super::{
    ReferenceDocument,
    topic_types::{CodeExampleTopicDoc, RuntimeTopicDoc},
};

pub(super) fn pubsub(doc: &ReferenceDocument) -> RuntimeTopicDoc {
    let mut signatures = vec![
        "type ChigiriSubscriptionPubSubInput = { topic: string, overflow: ChigiriSubscriptionOverflow? }".into(),
        "type ChigiriConsumerPubSubInput = { topic: string, all_topics: nil, overflow: ChigiriSubscriptionOverflow? } | { all_topics: true, topic: nil, overflow: ChigiriSubscriptionOverflow? }".into(),
        "type ChigiriSubscriptionOverflow = 'backpressure' | 'drop_oldest'".into(),
    ];
    for event in &doc.events.types {
        signatures.push(event.publish_input_definition.clone());
        signatures.push(event.publish_signature.clone());
        for source in &event.destinations {
            signatures.push(format!(
                "{}: {}",
                source.coordinate, source.pubsub_signature
            ));
            signatures.push(format!(
                "{} (named consumer): {}",
                source.coordinate, source.consumer_pubsub_signature
            ));
        }
    }
    RuntimeTopicDoc {
        name: "pubsub".into(),
        purpose: "SDL-type channels with post-commit publication and independently bounded sources.".into(),
        availability: vec![
            "Mutation resolve and named consume may publish in their own atomic write candidate".into(),
            "Subscription subscribe and named subscribe select only their destination's opaque source".into(),
            "all_topics=true is named-consumer-only; Query, nested resolvers and existing hooks do not gain publication".into(),
        ],
        signatures,
        limits: vec![
            "Exact signatures require schema/project/bundle input; only adopted Subscription output named types qualify, with no field aliases".into(),
            "Same-type publication captures once, counts once and broadcasts once to compatible complete destination shapes; preserve constraints and genuine Record provenance".into(),
            "Topics are exact case-sensitive 1..256 UTF-8 bytes without ASCII controls; '*' is literal; no replay or history".into(),
            "Independent source count/byte/age/shared limits apply by OR; backpressure stops only the affected source, drop_oldest evicts its wholly unprocessed inputs".into(),
            "Source bytes account retained source ownership, separately from encoded GraphQL output and process RSS".into(),
            "A consumer child adds one immutable host-owned depth; depths 1..3 are permitted and attempted depth 4 rolls back even through pcall".into(),
        ],
        examples: vec![CodeExampleTopicDoc {
            title: "Subscription setup body when its adopted output is Item".into(),
            code: "return ctx.pubsub.subscribe.Item({topic = 'items'})".into(),
        }],
    }
}

pub(super) fn generator(doc: &ReferenceDocument) -> RuntimeTopicDoc {
    let signatures = doc
        .events
        .types
        .iter()
        .flat_map(|event| &event.destinations)
        .flat_map(|source| {
            [
                format!("{}: {}", source.coordinate, source.generator_signature),
                source.sequence_input_definition.clone(),
            ]
        })
        .collect();
    RuntimeTopicDoc {
        name: "generator".into(),
        purpose: "Independent monotonic sequences from an immutable captured finite dataset.".into(),
        availability: vec![
            "Subscription subscribe and named subscribe only; only the returned opaque source activates".into(),
            "Exact destination-specific signatures require schema/project/bundle input".into(),
        ],
        signatures,
        limits: vec![
            "Closed values/interval_ms/loop?/overflow? input; 1..1000 dense complete outputs; interval_ms integer 1..86400000; loop defaults false".into(),
            "Each element is the whole field output, including a list or ctx.null when allowed; genuine Record capture never retains a VM or writable context".into(),
            "First emission is one interval after activation; independent fixed monotonic cadence ignores demand, apparent time and timeleap".into(),
            "Late wakes skip arithmetically to at most one latest due value; finite schedules emit the final value once and drain accepted inputs".into(),
            "Skipped ticks are not delivery drops; sequence topic is ctx.null and depth is zero; publication requires an explicit consumer bridge".into(),
            "Queue, definition and global retained-source bounds compose with the configured overflow policy; Graceful does not drain callback chains".into(),
        ],
        examples: vec![CodeExampleTopicDoc {
            title: "Subscription or named setup body for Item, with at least one seeded Item".into(),
            code: "return ctx.generator.sequence.Item({values = ctx.models.Item:find({}), interval_ms = 1000, loop = true})".into(),
        }],
    }
}
