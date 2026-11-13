//! The original completion token executes and is destroyed under short setup ownership.

use std::sync::Arc;

use chigiri_http::{ResponseCompletion, ResponseCompletionOutcome, ResponseParts};
use chigiri_runtime::RequestTelemetry;
use chigiri_runtime::instrumentation::InstrumentationScope;

pub(super) fn completion(
    token: Option<Arc<dyn ResponseCompletion>>,
    telemetry: &RequestTelemetry,
) -> Option<Arc<dyn ResponseCompletion>> {
    let Some(scope) = telemetry.instrumentation_scope() else {
        return token;
    };
    token.map(|token| {
        Arc::new(ScopedCompletion {
            token: Some(token),
            scope: scope.clone(),
        }) as Arc<dyn ResponseCompletion>
    })
}

struct ScopedCompletion {
    token: Option<Arc<dyn ResponseCompletion>>,
    scope: InstrumentationScope,
}

impl ResponseCompletion for ScopedCompletion {
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.scope.run(|| {
            self.token
                .as_ref()
                .expect("live initial completion")
                .complete(response, outcome)
        })
    }
}

impl Drop for ScopedCompletion {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            self.scope.run(|| drop(token));
        }
    }
}
