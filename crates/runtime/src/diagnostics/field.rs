//! Opt-in field scopes surround actual resolver polling and destruction.

use std::time::Instant;

use chigiri_core::executor::{FieldContext, FieldNext, Plugin};
use chigiri_core::{FieldResult, LifecyclePhase, LifecycleResult};

use super::{Diagnostics, request_identity};
use crate::instrumentation::InstrumentationKind;

pub(crate) struct FieldDiagnosticsPlugin {
    diagnostics: Diagnostics,
}

impl FieldDiagnosticsPlugin {
    pub(crate) const fn new(diagnostics: Diagnostics) -> Self {
        Self { diagnostics }
    }
}

impl<C> Plugin<C> for FieldDiagnosticsPlugin
where
    C: Send + Sync + 'static,
{
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        next: FieldNext<'a, C>,
    ) -> chigiri_core::executor::BoxFuture<'a, FieldResult<chigiri_core::executor::ResolvedValue>>
    {
        Box::pin(async move {
            let scope = context
                .request_telemetry()
                .and_then(|telemetry| telemetry.instrumentation_child(InstrumentationKind::Field));
            let finish = scope.clone();
            let work = async move {
                let started = Instant::now();
                let identity = context.identity();
                let telemetry = context.request_telemetry();
                let result = next.run(context).await;
                self.diagnostics.observe(
                    LifecyclePhase::Execution,
                    if result.is_ok() {
                        LifecycleResult::Succeeded
                    } else {
                        LifecycleResult::Failed
                    },
                    Some(started),
                    request_identity(identity, telemetry),
                    None,
                    "field",
                );
                if let Some(scope) = finish {
                    scope.finish(if result.is_ok() {
                        LifecycleResult::Succeeded
                    } else {
                        LifecycleResult::Failed
                    });
                }
                result
            };
            match scope.as_ref() {
                Some(scope) => scope.instrument(work).await,
                None => work.await,
            }
        })
    }
}
