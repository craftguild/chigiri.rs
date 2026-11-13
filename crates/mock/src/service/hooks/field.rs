//! Wrap field execution with before and after hooks while preserving the worker's result.

mod subscription;
#[cfg(test)]
pub(in crate::service) use subscription::exhaust_projection_for_test;
pub(in crate::service) use subscription::{establish_subscription, project_event};

use std::sync::{Arc, Mutex};

use chigiri::core::Object;
use chigiri::core::executor::ResolvedValue;
use chigiri::{FieldContext, FieldError, FieldNext, FieldResult};

use super::session::{self, FieldView, PhaseInput, Session};
use super::{error, views};
use crate::hooks::HookSlot;
use crate::ir::FieldId;
use crate::luau::ScriptError;
use crate::output::{OutputObject, OutputValue};
use crate::service::Client;
use crate::store::Snapshot;

tokio::task_local! { static FIELD: Arc<Frame>; }
struct Frame {
    origin: crate::limits::warning::LimitOrigin,
    field: FieldId,
    session: Arc<Session>,
    before: Option<Snapshot>,
    result: Mutex<Option<Snapshot>>,
}

pub(super) async fn run<'a>(
    context: FieldContext<'a, Client>,
    next: FieldNext<'a, Client>,
) -> FieldResult<ResolvedValue> {
    if context.field_name().starts_with("__") || context.parent_type().starts_with("__") {
        return next.run(context).await;
    }
    let client = context.context();
    let session = session::current();
    let mut progress = super::progress::After::new(&session.trace, client, HookSlot::FieldAfter);
    let graph = client.ir().graph();
    let parent = graph.type_id(context.parent_type()).ok_or_else(failed)?;
    let field = *graph.types()[parent.0]
        .fields
        .get(context.field_name())
        .ok_or_else(failed)?;
    if !session.active(client) {
        return Err(failed());
    }
    let client = session.client.get().unwrap_or(client);
    let watched = client.enabled(HookSlot::FieldBefore)
        || (client.enabled(HookSlot::FieldAfter) && !session.trace.observations_stopped());
    let view = if watched {
        super::copy_budget::at(
            session
                .trace
                .view_budget(client.enabled(HookSlot::FieldBefore)),
            crate::limits::warning::LimitOrigin {
                phase: "field.view",
                coordinate: Some(coordinate(client, field).into()),
                path: context.path().to_vec().into(),
            },
            |budget| views::field(&context, budget),
        )
        .ok()
    } else {
        None
    };
    let before = match context.parent() {
        Some(parent) => parent
            .downcast_ref::<OutputObject>()
            .map(|parent| Some(parent.snapshot().clone()))
            .ok_or_else(|| error("mock parent required")),
        None => client.snapshot(session.trace.deadline).await.map(Some),
    };
    let frame = Arc::new(Frame {
        origin: crate::limits::warning::LimitOrigin {
            phase: "resolver",
            coordinate: Some(coordinate(client, field).into()),
            path: context.path().to_vec().into(),
        },
        field,
        session: session.clone(),
        before: before.as_ref().ok().cloned().flatten(),
        result: Mutex::new(None),
    });
    let proceed = match before {
        Ok(snapshot) => session
            .before(
                client,
                HookSlot::FieldBefore,
                PhaseInput {
                    field: view.clone(),
                    snapshot,
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| {
                let mut field = FieldError::public(error.message);
                for (name, value) in error.extensions {
                    field = field.extension(name, value);
                }
                field
            }),
        Err(error) => {
            session.diagnostic(
                HookSlot::FieldBefore,
                &view
                    .as_ref()
                    .map(|view| view.location.clone())
                    .unwrap_or_default(),
                error,
            );
            Err(failed())
        }
    };
    let result = match proceed {
        Ok(()) => FIELD.scope(frame.clone(), next.run(context)).await,
        Err(error) => Err(error),
    };
    after(client, &frame, view, &result, &mut progress).await;
    result
}

async fn after(
    client: &Client,
    frame: &Frame,
    view: Option<FieldView>,
    result: &FieldResult<ResolvedValue>,
    progress: &mut super::progress::After,
) {
    let session = &frame.session;
    if !client.enabled(HookSlot::FieldAfter)
        || !session.active(client)
        || session.trace.observations_stopped()
    {
        return;
    }
    let Some(view) = view else {
        session.diagnostic(
            HookSlot::FieldAfter,
            &Default::default(),
            error("field snapshot conversion failed"),
        );
        return;
    };
    let observed = super::copy_budget::at(
        session.trace.observations(),
        crate::limits::warning::LimitOrigin {
            phase: "field.after",
            coordinate: view.location.coordinate.clone(),
            path: view.location.path.clone(),
        },
        |budget| super::observation::outcome(result, &view.location.path, budget),
    );
    let snapshot = frame
        .result
        .lock()
        .expect("field result generation lock")
        .clone()
        .or_else(|| frame.before.clone());
    match observed {
        Ok(observed) => {
            session
                .after(
                    client,
                    HookSlot::FieldAfter,
                    PhaseInput {
                        field: Some(view),
                        result: Some(Arc::new(observed)),
                        snapshot,
                    },
                    progress,
                )
                .await
        }
        Err(_) => session.diagnostic(
            HookSlot::FieldAfter,
            &view.location,
            error("field outcome observation failed"),
        ),
    }
}

pub(in crate::service) async fn resolve(
    client: &Client,
    field: FieldId,
    arguments: Object,
    parent: Option<OutputObject>,
) -> Result<OutputValue, ScriptError> {
    if let Ok(frame) = FIELD.try_with(Arc::clone) {
        if frame.field != field {
            return Err(error("field invocation differs from dispatch"));
        }
        let client = frame.session.client.get().unwrap_or(client);
        let mut execution = frame.session.execution(client)?;
        execution.budget = execution.budget.at(frame.origin.clone());
        let diagnosed = diagnosed(client, &frame);
        let result = match diagnosed
            .execute_with(field, arguments, parent, frame.before.clone(), execution)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                diagnostic(client, &frame, &error);
                return Err(error);
            }
        };
        if let Err(error) = &result.output {
            diagnostic(client, &frame, error);
        }
        *frame.result.lock().expect("fieldResult generation") = Some(result.snapshot);
        result.output
    } else {
        client.execute(field, arguments, parent).await
    }
}

fn diagnosed(client: &Client, frame: &Frame) -> Client {
    let trace = frame.session.trace.clone();
    let field = frame.field;
    let coordinate = coordinate(client, field);
    let module = resolver_module(client, field);
    client.with_diagnostics(move |error| {
        trace.resolver_diagnostic(field, coordinate.clone(), module.clone(), error);
    })
}

fn failed() -> FieldError {
    FieldError::public("Mock hook failed").code("MOCK_HOOK_ERROR")
}

fn diagnostic(client: &Client, frame: &Frame, error: &ScriptError) {
    if error.phase == "business" {
        return;
    }
    frame.session.execution_error(error);
    frame.session.trace.resolver_diagnostic(
        frame.field,
        coordinate(client, frame.field),
        resolver_module(client, frame.field),
        error.clone(),
    );
}

fn coordinate(client: &Client, field: FieldId) -> String {
    let graph = client.ir().graph();
    let field = &graph.fields()[field.0];
    format!("{}.{}", graph.types()[field.parent.0].name, field.name)
}

fn resolver_module(client: &Client, field: FieldId) -> Option<String> {
    let ir = client.ir();
    if !matches!(ir.field_plan(field), Some(crate::ir::FieldPlan::Script(_))) {
        return None;
    }
    let graph = ir.graph();
    let field = &graph.fields()[field.0];
    let parent = &graph.types()[field.parent.0].name;
    let owner = if field.parent == graph.query() {
        "query".to_owned()
    } else if Some(field.parent) == graph.mutation() {
        "mutation".to_owned()
    } else {
        format!("objects/{parent}")
    };
    Some(format!("resolvers/{owner}/{}.luau", field.name))
}
