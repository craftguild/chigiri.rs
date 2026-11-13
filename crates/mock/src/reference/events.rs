//! Append adopted type selectors and configured registrations under the shared document budget.

use super::{ReferenceError, budget::Budget, event_types::*};
use crate::events::EventSettings;
use crate::ir::{FieldPlan, MockIr};

pub(super) fn empty(settings: Option<&EventSettings>) -> EventsDoc {
    EventsDoc {
        configured: settings.is_some(),
        limits: settings.map(EventSettings::limits).unwrap_or_default(),
        types: Vec::new(),
        consumers: Vec::new(),
    }
}

pub(super) fn append(
    doc: &mut EventsDoc,
    ir: &MockIr,
    settings: Option<&EventSettings>,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    for event in crate::events::payload::event_types(ir) {
        let contract = crate::luau::publication_contract(ir, event.named, &event.fields)
            .map_err(|_| ReferenceError::contract())?;
        let mut item = EventTypeDoc {
            name: ir.graph().types()[event.named.0].name.to_string(),
            publish_signature: contract.signature,
            publish_input_definition: contract.definition,
            destinations: Vec::new(),
        };
        // Reserve each nested row before retaining it, then charge the complete
        // item once. A large fan-out cannot bypass the document's 16 MiB cap.
        let mut reserved = serde_json::to_vec(&item)
            .map_err(|_| ReferenceError::contract())?
            .len();
        for field in event.fields {
            let Some(FieldPlan::Script(id)) = ir.field_plan(field) else {
                return Err(ReferenceError::contract());
            };
            let contract =
                crate::luau::source_contract(ir, *id).map_err(|_| ReferenceError::contract())?;
            let node = &ir.graph().fields()[field.0];
            let destination = EventDestinationDoc {
                coordinate: format!("{}.{}", ir.graph().types()[node.parent.0].name, node.name),
                description: ir
                    .graph()
                    .field_source(field)
                    .and_then(|field| field.description.as_ref())
                    .map(|value| value.value.value.clone()),
                payload_type: contract.payload_type,
                source_type: contract.source_type,
                pubsub_signature: contract.pubsub,
                consumer_pubsub_signature: contract.consumer_pubsub,
                generator_signature: contract.generator,
                sequence_input_definition: contract.sequence_definition,
                graphql_subscribe_signature: contract.graphql_subscribe,
                consumer_subscribe_signature: contract.consumer_subscribe,
                consume_signature: contract.consume,
                constraints: super::schema_values::constraints(&node.constraints),
            };
            budget.reserve_array_item(&mut reserved, &destination)?;
            item.destinations.push(destination);
        }
        budget.push(&mut doc.types, item)?;
    }
    if let Some(settings) = settings {
        for registration in settings
            .resolve(ir)
            .map_err(|_| ReferenceError::contract())?
        {
            let field = &ir.graph().fields()[registration.field().0];
            let coordinate = format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name);
            let source = doc
                .types
                .iter()
                .flat_map(|event| &event.destinations)
                .find(|source| source.coordinate == coordinate)
                .ok_or_else(ReferenceError::contract)?;
            budget.push(&mut doc.consumers, ConsumerDoc {
                name: registration.name().to_owned(),
                enabled: registration.enabled(),
                module_path: registration.path().clone(),
                source: source.clone(),
                setup_authority: "read models; log; select one exact-topic, all-topic or sequence source; no GraphQL arguments or publication".into(),
                execution_authority: "fresh null-request write candidate; log and typed publish; atomic commit; host-owned maximum three republish edges".into(),
            })?;
        }
    }
    Ok(())
}
