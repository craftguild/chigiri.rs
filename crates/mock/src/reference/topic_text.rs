//! Render the short topic index and exact topic details as plain text.

use super::{ReferenceDocument, man, selection::Topic, topic_types::*};
use std::io::{self, Write};

pub(super) fn landing(doc: &ReferenceDocument, out: &mut impl Write) -> io::Result<()> {
    man::title(out)?;
    man::identity(
        out,
        "Display a Mock API reference topic or named item.",
        super::input_text::landing_synopsis(doc.input_kind),
    )?;
    man::section(out, "DESCRIPTION")?;
    writeln!(
        out,
        "    Select an item to show its details. input_kind={}, readiness_checked=false. {}",
        doc.input_kind.name(),
        super::input_text::boundary(doc.input_kind)
    )?;
    man::section(out, "TOPICS")?;
    writeln!(
        out,
        "directives GraphQL standard, Mock-owned, and user-declared directives"
    )?;
    writeln!(
        out,
        "methods     seven model operations and their read/write authority"
    )?;
    writeln!(
        out,
        "hooks       six before/after hooks with state, rejection, and logging behavior"
    )?;
    writeln!(
        out,
        "models      schema models and their available operations"
    )?;
    writeln!(
        out,
        "types       GraphQL types, fields, arguments, and relationships"
    )?;
    writeln!(
        out,
        "resolvers   resolver modules, signatures, and capabilities"
    )?;
    writeln!(
        out,
        "limits      runtime resources, including soft and hard request limits"
    )?;
    writeln!(
        out,
        "runtime     executable stdlib, request, timeleap, errors, storage, pubsub, and generator surfaces"
    )?;
    writeln!(
        out,
        "all         complete reference, including ID-based type graphs"
    )?;
    writeln!(
        out,
        "consumers   registered named consumers, including disabled modules"
    )?;
    man::section(out, "EXAMPLES")?;
    writeln!(
        out,
        "    chigiri mock reference runtime timeleap{}",
        super::input_text::input_option(doc.input_kind)
    )?;
    writeln!(
        out,
        "    chigiri mock reference types User{}",
        super::input_text::input_option(doc.input_kind)
    )?;
    writeln!(
        out,
        "    chigiri mock reference resolvers Query.user{}",
        super::input_text::input_option(doc.input_kind)
    )?;
    man::section(out, "SEE ALSO")?;
    writeln!(out, "    chigiri mock reference all")
}

pub(super) fn render(document: &TopicDocument, out: &mut impl Write) -> io::Result<()> {
    man::title(out)?;
    render_body(document, out)
}

fn render_body(document: &TopicDocument, out: &mut impl Write) -> io::Result<()> {
    if document.selection.name.is_some() {
        return super::topic_text_detail::render(&document.content, true, document.input_kind, out);
    }
    let topic = document.selection.topic;
    man::identity(
        out,
        topic_purpose(topic),
        super::input_text::selection_synopsis(
            document.input_kind,
            topic.name(),
            topic.accepts_name().then_some("[NAME]"),
        ),
    )?;
    man::section(out, "DESCRIPTION")?;
    writeln!(
        out,
        "    {} {} readiness_checked=false.",
        topic_description(topic),
        super::input_text::boundary(document.input_kind)
    )?;
    man::section(out, "ITEMS")?;
    super::topic_text_detail::render(&document.content, false, document.input_kind, out)?;
    man::see_also(out)
}

fn topic_purpose(topic: Topic) -> &'static str {
    match topic {
        Topic::Directives => "GraphQL directive catalog.",
        Topic::Methods => "Mock model method catalog.",
        Topic::Hooks => "Mock hook catalog.",
        Topic::Models => "Models derived from the input schema.",
        Topic::Types => "GraphQL types derived from the input schema.",
        Topic::Resolvers => "Resolver contracts derived from the input schema.",
        Topic::Limits => "Mock runtime limits.",
        Topic::Runtime => "Executable Mock runtime surface.",
        Topic::Consumers => "Registered named Mock consumers.",
        Topic::All => "Complete Mock API reference.",
    }
}

fn topic_description(topic: Topic) -> &'static str {
    match topic {
        Topic::Directives => {
            "Lists standard GraphQL directives, Mock directives, and user declarations."
        }
        Topic::Methods => "Lists the seven model operations and their read/write permissions.",
        Topic::Hooks => "Lists the six before/after hooks and their state and rejection behavior.",
        Topic::Models => "Lists schema models and their available operations.",
        Topic::Types => "Lists schema types, fields, arguments, and relationships.",
        Topic::Resolvers => "Lists resolver modules, signatures, scopes, and model capabilities.",
        Topic::Limits => "Lists runtime resource limits, including soft and hard thresholds.",
        Topic::Runtime => "Lists executable signatures, availability, limits, and examples.",
        Topic::Consumers => {
            "Lists configured consumer modules, destinations, signatures and explicit phase authority."
        }
        Topic::All => "Shows the complete reference, including the ID-based type graph.",
    }
}
