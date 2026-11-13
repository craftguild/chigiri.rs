//! Derive stable public type names from semantic SDL units and reject reserved or case-folding collisions.

use std::collections::BTreeSet;

use chigiri::core::schema::TypeKind;

use super::ScriptError;
use crate::ir::{MockIr, ScriptId, TypeId};

fn title(name: &str) -> String {
    let mut name = name.to_owned();
    // GraphQL names are ASCII; capitalize only the first byte and isolate reserved words behind the Chigiri prefix.
    name[..1].make_ascii_uppercase();
    name
}

pub(super) fn ty(ir: &MockIr, id: TypeId, suffix: &str) -> String {
    format!("Chigiri{}{suffix}", title(&ir.graph().types()[id.0].name))
}

pub(super) fn resolver(ir: &MockIr, id: ScriptId, suffix: &str) -> String {
    let plan = &ir.scripts()[id.0];
    let field = &ir.graph().fields()[plan.field.0];
    let owner = if field.parent == ir.graph().query() {
        "Query".to_owned()
    } else if Some(field.parent) == ir.graph().mutation() {
        "Mutation".to_owned()
    } else if Some(field.parent) == ir.graph().subscription() {
        title(&ir.graph().types()[field.parent.0].name)
    } else {
        format!("Object{}", title(&ir.graph().types()[field.parent.0].name))
    };
    format!("Chigiri{owner}{}{suffix}", title(&field.name))
}

pub(super) fn method(ir: &MockIr, id: TypeId, method: &str) -> String {
    ty(ir, id, &format!("{}Method", title(method)))
}

pub(super) fn validate(ir: &MockIr) -> Result<(), ScriptError> {
    let mut names = BTreeSet::new();
    let mut insert = |name: String| {
        if names.insert(name.to_ascii_lowercase()) {
            Ok(())
        } else {
            Err(ScriptError::new("contract", "public type name collision"))
        }
    };
    for name in [
        "ChigiriNull",
        "ChigiriJson",
        "ChigiriJsonValue",
        "ChigiriJsonObject",
        "ChigiriJsonArray",
        "ChigiriReadContext",
        "ChigiriWriteContext",
        "ChigiriMockProjectConfig",
        "ChigiriStateKind",
        "ChigiriConfigValue",
        "ChigiriHookRegistration",
        "ChigiriClockConfig",
        "ChigiriEventLimits",
        "ChigiriConsumerRegistration",
        "ChigiriEventSettings",
        "ChigiriState",
        "ChigiriStatePatch",
        "ChigiriHookLog",
        "ChigiriHookReject",
    ] {
        insert(name.to_owned())?;
    }
    if ir.graph().subscription().is_some() {
        for name in [
            "ChigiriSubscriptionOverflow",
            "ChigiriSubscriptionPubSubInput",
            "ChigiriConsumerPubSubInput",
            "ChigiriConsumerTopicInput",
            "ChigiriConsumerAllTopicsInput",
            "ChigiriConsumerEventMetadata",
        ] {
            insert(name.to_owned())?;
        }
    }
    for shape in crate::api::hooks::SNAPSHOTS {
        insert(shape.alias.to_owned())?;
    }
    for slot in crate::hooks::HookSlot::ALL {
        for suffix in ["Context", "Result", "Hook"] {
            insert(crate::api::hooks::alias(slot, suffix))?;
        }
    }
    for (id, node) in ir.graph().types().iter().enumerate() {
        let id = TypeId(id);
        let suffixes: &[&str] = match node.kind {
            TypeKind::Scalar | TypeKind::Enum => &["Value"],
            TypeKind::InputObject => &["Input"],
            TypeKind::Object if ir.model_for(id).is_some() => &[
                "Stored",
                "ConcreteStored",
                "Record",
                "ConcreteRecord",
                "CreateInput",
                "UpdateInput",
                "Where",
                "OrderKey",
                "FindOptions",
                "CountOptions",
                "ReadModel",
                "WriteModel",
            ],
            TypeKind::Object if Some(id) == ir.graph().subscription() => {
                &["Stored", "ConcreteStored"]
            }
            TypeKind::Object => &["Object", "Stored", "ConcreteStored"],
            TypeKind::Interface | TypeKind::Union => &["Abstract", "StoredAbstract"],
        };
        for suffix in suffixes {
            insert(ty(ir, id, suffix))?;
        }
        if Some(id) == ir.graph().subscription() {
            insert(ty(ir, id, ""))?;
        }
        if let Some(model) = ir.model_for(id) {
            if ir.relation_fields(model).next().is_some() {
                insert(ty(ir, id, "Relation"))?;
            }
            for spec in crate::api::METHODS {
                if spec.eligibility.allows(ir, model) {
                    insert(method(ir, id, spec.name))?;
                }
            }
        }
    }
    for event in crate::events::payload::event_types(ir) {
        insert(ty(ir, event.named, "EventPublishInput"))?;
    }
    for (id, _) in ir.scripts().iter().enumerate() {
        let suffixes: &[&str] = match ir.scripts()[id].role {
            crate::ir::ScriptRole::SubscriptionRoot {
                root_as_nested: true,
            } => &[
                "Args",
                "Source",
                "SetupContext",
                "SequenceInput",
                "ConsumerSetupContext",
                "ConsumerContext",
                "Event",
                "Consumer",
                "Result",
                "Resolver",
            ],
            crate::ir::ScriptRole::SubscriptionRoot {
                root_as_nested: false,
            } => &[
                "Args",
                "Source",
                "SetupContext",
                "SequenceInput",
                "ConsumerSetupContext",
                "ConsumerContext",
                "Event",
                "Consumer",
                "Resolver",
            ],
            _ => &["Args", "Result", "Context", "Resolver"],
        };
        for suffix in suffixes {
            insert(resolver(ir, ScriptId(id), suffix))?;
        }
    }
    Ok(())
}
