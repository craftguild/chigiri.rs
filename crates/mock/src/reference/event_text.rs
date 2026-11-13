//! Render safe consumer discovery and the full event contract without runtime values.

use std::io::{self, Write};

use super::{
    InputKind,
    event_types::{ConsumerDoc, EventsDoc},
    man,
};

pub(super) fn consumers(
    items: &[ConsumerDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(
            out,
            "    No configured consumers. Use --config-dir or --config for registered modules, including disabled entries."
        );
    }
    if !detail {
        for item in items {
            writeln!(
                out,
                "    {} — {} enabled={} [{}]",
                item.name, item.source.coordinate, item.enabled, item.module_path
            )?;
        }
        return Ok(());
    }
    let item = &items[0];
    super::input_text::detail_header(
        out,
        "Display one registered transactional consumer.",
        input,
        "consumers",
        &item.name,
    )?;
    writeln!(
        out,
        "    module: {}  field: {}  enabled={}",
        item.module_path, item.source.coordinate, item.enabled
    )?;
    if let Some(description) = &item.source.description {
        write!(out, "    description: ")?;
        super::text::visible(out, description)?;
        writeln!(out)?;
    }
    man::section(out, "SIGNATURES")?;
    for signature in [
        &item.source.consumer_subscribe_signature,
        &item.source.consume_signature,
        &item.source.consumer_pubsub_signature,
        &item.source.generator_signature,
        &item.source.sequence_input_definition,
    ] {
        writeln!(out, "    {signature}")?;
    }
    writeln!(
        out,
        "    payload: {}\n    constraints: {}",
        item.source.payload_type,
        super::text::json(&item.source.constraints)?
    )?;
    man::section(out, "AUTHORITY")?;
    writeln!(
        out,
        "    subscribe: {}\n    consume: {}",
        item.setup_authority, item.execution_authority
    )?;
    writeln!(
        out,
        "    ctx.request is ctx.null; ctx.event.topic/depth/droppedTotal are read-only host metadata"
    )?;
    writeln!(
        out,
        "    Disabled modules are checked but never invoked; reference does not check readiness or start sources"
    )?;
    man::see_also(out)
}

pub(super) fn full(events: &EventsDoc, input: InputKind, out: &mut impl Write) -> io::Result<()> {
    man::section(out, "EVENT SOURCES")?;
    writeln!(
        out,
        "    configured={} limits={}",
        events.configured,
        super::text::json(&events.limits)?
    )?;
    for event in &events.types {
        writeln!(
            out,
            "    {}\n    {}",
            event.publish_signature, event.publish_input_definition
        )?;
        for source in &event.destinations {
            writeln!(
                out,
                "    {}: {}\n        {}\n        {}",
                source.coordinate,
                source.payload_type,
                source.pubsub_signature,
                source.generator_signature
            )?;
        }
    }
    man::section(out, "CONSUMERS")?;
    consumers(&events.consumers, false, input, out)
}
