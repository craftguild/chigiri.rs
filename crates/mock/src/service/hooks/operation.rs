//! Apply the same six Mock hook phases within finite, setup, or fresh event ownership.

use std::sync::Arc;

use chigiri::{GraphQlResponse, OperationContext, OperationNext};

use super::{copy_budget, error, progress, session, views};
use crate::hooks::HookSlot;
use crate::service::Client;

pub(super) async fn run<'a>(
    context: OperationContext<'a, Client>,
    next: OperationNext<'a, Client>,
) -> GraphQlResponse {
    let session = session::current();
    let client = context.context();
    let mut request_after = progress::After::new(&session.trace, client, HookSlot::RequestAfter);
    let mut operation_after =
        progress::After::new(&session.trace, client, HookSlot::OperationAfter);
    let mutation =
        context.operation_type() == chigiri::core::language::ast::OperationType::Mutation;
    if !session.start(client, mutation).await {
        return GraphQlResponse::request_error(chigiri::GraphQlError::new("Mock execution failed"));
    }
    if let Err(error) = session
        .before(client, HookSlot::RequestBefore, Default::default())
        .await
    {
        return GraphQlResponse::request_error(error);
    }
    if [
        HookSlot::OperationBefore,
        HookSlot::OperationAfter,
        HookSlot::FieldBefore,
        HookSlot::FieldAfter,
    ]
    .into_iter()
    .any(|slot| client.enabled(slot))
    {
        let view = copy_budget::at(
            session.trace.view_budget(
                client.enabled(HookSlot::OperationBefore) || client.enabled(HookSlot::FieldBefore),
            ),
            crate::limits::warning::LimitOrigin {
                phase: "operation.view",
                ..Default::default()
            },
            |budget| views::operation(&context, budget),
        );
        let _ = session.operation.set(
            view.map(Arc::new)
                .map_err(|_| error("operation snapshot conversion failed")),
        );
    }
    let prepared = session
        .before(client, HookSlot::OperationBefore, Default::default())
        .await;
    session.trace.execution_budget.seal_clock();
    let response = match prepared {
        Ok(()) => next.run(context).await,
        Err(error) => GraphQlResponse::request_error(error),
    };
    session
        .after_response(
            client,
            HookSlot::OperationAfter,
            &response,
            &mut operation_after,
        )
        .await;
    session
        .after_response(
            client,
            HookSlot::RequestAfter,
            &response,
            &mut request_after,
        )
        .await;
    response
}
