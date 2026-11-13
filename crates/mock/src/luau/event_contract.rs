//! Derive discoverable signatures from the same names and output renderer as declarations.

use super::{ScriptError, names, reference, text::Text};
use crate::ir::{FieldId, MockIr, ScriptId, TypeId};

pub(crate) struct PublicationContract {
    pub signature: String,
    pub definition: String,
}

pub(crate) struct SourceContract {
    pub source_type: String,
    pub payload_type: String,
    pub pubsub: String,
    pub consumer_pubsub: String,
    pub generator: String,
    pub sequence_definition: String,
    pub graphql_subscribe: String,
    pub consumer_subscribe: String,
    pub consume: String,
}

pub(crate) fn publication_contract(
    ir: &MockIr,
    named: TypeId,
    fields: &[FieldId],
) -> Result<PublicationContract, ScriptError> {
    let mut out = Text::new(super::MAX_GENERATED);
    out.format(format_args!(
        "type {} = {{ topic: string, payload: ",
        names::ty(ir, named, "EventPublishInput")
    ))?;
    super::subscription::declarations::publication_payload(&mut out, ir, fields)?;
    out.add(" }\n")?;
    Ok(PublicationContract {
        signature: format!(
            "ctx.pubsub.publish.{}(input: {}) -> ()",
            ir.graph().types()[named.0].name,
            names::ty(ir, named, "EventPublishInput")
        ),
        definition: out.finish(),
    })
}

pub(crate) fn source_contract(ir: &MockIr, id: ScriptId) -> Result<SourceContract, ScriptError> {
    if !ir.scripts()[id.0].role.is_subscription_root() {
        return Err(ScriptError::new(
            "contract",
            "subscription source contract required",
        ));
    }
    let field = ir.scripts()[id.0].field;
    let selector = crate::events::payload::field_type_name(ir, field);
    let source_type = names::resolver(ir, id, "Source");
    let mut out = Text::new(super::MAX_GENERATED);
    reference::reference(
        &mut out,
        ir,
        &ir.graph().fields()[field.0].ty,
        reference::Mode::Output,
    )?;
    let payload_type = out.finish();
    let mut out = Text::new(super::MAX_GENERATED);
    super::subscription::declarations::sequence_input(&mut out, ir, id)?;
    Ok(SourceContract {
        pubsub: format!(
            "ctx.pubsub.subscribe.{selector}(input: ChigiriSubscriptionPubSubInput) -> {source_type}"
        ),
        consumer_pubsub: format!(
            "ctx.pubsub.subscribe.{selector}(input: ChigiriConsumerPubSubInput) -> {source_type}"
        ),
        generator: format!(
            "ctx.generator.sequence.{selector}(input: {}) -> {source_type}",
            names::resolver(ir, id, "SequenceInput")
        ),
        sequence_definition: out.finish(),
        graphql_subscribe: format!(
            "subscribe(ctx: {}, args: {}) -> {source_type}",
            names::resolver(ir, id, "SetupContext"),
            names::resolver(ir, id, "Args")
        ),
        consumer_subscribe: format!(
            "subscribe(ctx: {}) -> {source_type}",
            names::resolver(ir, id, "ConsumerSetupContext")
        ),
        consume: format!(
            "consume(ctx: {}, event: {}) -> ()",
            names::resolver(ir, id, "ConsumerContext"),
            names::resolver(ir, id, "Event")
        ),
        source_type,
        payload_type,
    })
}
