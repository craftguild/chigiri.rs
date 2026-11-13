//! Generate field-specific background consumer roles from public Subscription fields.

use super::ScriptError;
use super::names;
use super::reference::{self, Mode};
use super::text::Text;
use crate::api::{consumers as contract, context, subscriptions};
use crate::ir::{MockIr, ScriptId};

pub(super) fn generate(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    if ir.graph().subscription().is_none() {
        return Ok(());
    }
    out.add("type ChigiriConsumerTopicInput = {\n")?;
    out.key(subscriptions::TOPIC)?;
    out.add("string,\n")?;
    out.key(contract::ALL_TOPICS)?;
    out.add("nil,\n")?;
    overflow(out)?;
    out.add("}\ntype ChigiriConsumerAllTopicsInput = {\n")?;
    out.key(contract::ALL_TOPICS)?;
    out.add("true,\n")?;
    out.key(subscriptions::TOPIC)?;
    out.add("nil,\n")?;
    overflow(out)?;
    out.add("}\ntype ChigiriConsumerPubSubInput = ChigiriConsumerTopicInput | ChigiriConsumerAllTopicsInput\ntype ChigiriConsumerEventMetadata = {\n")?;
    out.key(subscriptions::TOPIC)?;
    out.add("string | ChigiriNull,\n")?;
    out.key(contract::DEPTH)?;
    out.add("number,\n")?;
    out.key(contract::DROPPED_TOTAL)?;
    out.add("number | ChigiriNull,\n}\n")?;
    for (index, plan) in ir.scripts().iter().enumerate() {
        if !plan.role.is_subscription_root() {
            continue;
        }
        let id = ScriptId(index);
        out.format(format_args!("type {} = ", names::resolver(ir, id, "Event")))?;
        reference::reference(out, ir, &ir.graph().fields()[plan.field.0].ty, Mode::Output)?;
        out.add("\n")?;
        context_type(out, ir, id, true)?;
        context_type(out, ir, id, false)?;
        out.format(format_args!(
            "type {} = {{\n",
            names::resolver(ir, id, "Consumer")
        ))?;
        out.key(contract::SUBSCRIBE)?;
        out.format(format_args!(
            "(ctx: {}) -> {},\n",
            names::resolver(ir, id, "ConsumerSetupContext"),
            names::resolver(ir, id, "Source")
        ))?;
        out.key(contract::CONSUME)?;
        out.format(format_args!(
            "(ctx: {}, event: {}) -> (),\n}}\n",
            names::resolver(ir, id, "ConsumerContext"),
            names::resolver(ir, id, "Event")
        ))?;
    }
    Ok(())
}

fn overflow(out: &mut Text) -> Result<(), ScriptError> {
    out.key(subscriptions::OVERFLOW)?;
    out.add("ChigiriSubscriptionOverflow?,\n")
}

fn context_type(out: &mut Text, ir: &MockIr, id: ScriptId, setup: bool) -> Result<(), ScriptError> {
    let (suffix, scope, model) = if setup {
        ("ConsumerSetupContext", contract::SETUP_SCOPE, "ReadModel")
    } else {
        ("ConsumerContext", contract::EXECUTION_SCOPE, "WriteModel")
    };
    out.format(format_args!(
        "type {} = {{\n",
        names::resolver(ir, id, suffix)
    ))?;
    out.key(context::SCOPE)?;
    out.quoted(scope)?;
    out.add(",\n")?;
    super::models::common_context_with_request(out, ir, model, "ChigiriNull")?;
    out.key(crate::api::hook_values::LOG_FUNCTION.name)?;
    out.add("ChigiriHookLog,\n")?;
    if setup {
        super::subscription::declarations::source_privilege(
            out,
            ir,
            id,
            "ChigiriConsumerPubSubInput",
        )?;
    } else {
        out.key(contract::EVENT)?;
        out.add("ChigiriConsumerEventMetadata,\n")?;
        super::subscription::declarations::write_privilege(out, ir)?;
    }
    out.add("}\n")
}
