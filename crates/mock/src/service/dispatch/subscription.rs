//! Adapt real Mock host bindings and immutable events to the common runtime.

use std::sync::Arc;

use chigiri::__private::{
    ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent,
    HostSubscriptionObservation, SubscriptionObservationReceipt, SubscriptionResolverDispatch,
    erase_subscription_source,
};
use chigiri::core::executor::{ArgumentValues, ResolvedValue};
use chigiri::{BoxFuture, FieldContext, FieldError, FieldResult, SubscriptionSource};

use super::{Client, Dispatch, contract, convert};
use crate::publication::CapturedEvent;

impl SubscriptionResolverDispatch<Client> for Dispatch {
    fn prepare_observation(
        &self,
        client: &Client,
        parent_type: &str,
        field_name: &str,
        receipt: SubscriptionObservationReceipt,
        remaining_metadata_bytes: usize,
    ) -> FieldResult<Option<Box<dyn HostSubscriptionObservation>>> {
        let field = self.field(client, parent_type, field_name)?;
        if Some(self.ir.graph().fields()[field.0].parent) != self.ir.graph().subscription() {
            return Err(contract());
        }
        super::super::hooks::prepare_subscription_observation(
            client,
            parent_type,
            field_name,
            receipt,
            remaining_metadata_bytes,
        )
    }

    fn event_execution_failed(&self, client: &Client) -> bool {
        client
            .session
            .as_ref()
            .is_some_and(|session| session.event_execution_failed())
    }

    fn subscription_root(&self) -> Option<&str> {
        let graph = self.ir.graph();
        graph
            .subscription()
            .map(|root| graph.types()[root.0].name.as_ref())
    }

    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, Client>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        Box::pin(async move {
            let field = self.field(
                context.context(),
                context.parent_type(),
                context.field_name(),
            )?;
            if Some(self.ir.graph().fields()[field.0].parent) != self.ir.graph().subscription() {
                return Err(contract());
            }
            let arguments = self.arguments(field, arguments)?;
            let registered =
                super::super::hooks::establish_subscription(context.context(), field, arguments)
                    .await
                    .map_err(|error| self.map_error(field, error))?;
            let super::super::hooks::EstablishedParts {
                source,
                overflow,
                observation,
            } = registered
                .into_parts()
                .map_err(|error| self.map_error(field, error))?;
            let report = Arc::clone(&self.report);
            let erased = erase_subscription_source(
                SubscriptionSource::new(source).with_overflow(overflow),
                (),
                Arc::new(move |error| source_error(report.as_ref(), field, error)),
            );
            match observation {
                Some(receipt) => erased.with_host_observation(receipt),
                None => Ok(erased),
            }
        })
    }

    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, Client>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            arguments.downcast_ref::<()>()?;
            let field = self.field(
                context.context(),
                context.parent_type(),
                context.field_name(),
            )?;
            let event = event.downcast_ref::<CapturedEvent>()?;
            let output = super::super::hooks::project_event(context.context(), field, event)
                .map_err(|error| self.map_error(field, error))?;
            convert(self.ir.public_graph().source(), output)
        })
    }
}

fn source_error(
    report: &super::Report,
    field: crate::ir::FieldId,
    error: crate::luau::ScriptError,
) -> FieldError {
    // Host source failures run outside a fresh event Session. Retain one mapper
    // result for the normal internal observer, never its public diagnostic text.
    let reported = report(field, error);
    if reported.is_internal() {
        reported
    } else {
        FieldError::internal(reported)
    }
}

#[cfg(test)]
mod tests;
