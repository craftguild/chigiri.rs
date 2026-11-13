//! Actual-upgrade handoff that preserves the original admission deadline.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{ConnectionInitAuthorizer, DeadlineFactory};
use chigiri_runtime::instrumentation::{InstrumentationKind, InstrumentationScope};

use super::super::ClaimedConnectionParts;
use super::authorization::AuthorizationErrorObserver;
use super::cleanup::{self, WebSocketConnectionCleanup};
use super::close::WebSocketConnectionClose;
use super::initialization::WebSocketConnectionInitialization;
use crate::subscription::control::WebSocketControlStaging;

const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(10);

/// First application-driven poll after the HTTP upgrade was accepted.
#[must_use = "the upgraded connection handoff must be polled or closed"]
#[doc(hidden)]
pub struct WebSocketConnectionHandoff {
    parts: Option<ClaimedConnectionParts>,
    initialization_timer: Option<chigiri_http::BoxFuture<'static, ()>>,
    deadlines: &'static dyn DeadlineFactory,
    authorizer: Arc<dyn ConnectionInitAuthorizer>,
    observer: Arc<dyn AuthorizationErrorObserver>,
    activity: crate::managed::EventActivity,
}

/// Result of transferring the admitted upgrade into protocol initialization.
#[doc(hidden)]
pub enum WebSocketConnectionHandoffResolution {
    /// Both handoff deadlines were pending, so initialization may begin.
    Initializing(Box<WebSocketConnectionInitialization>),
    /// The original admission deadline had already expired after HTTP 101.
    Closing(WebSocketConnectionCleanup),
}

impl WebSocketConnectionHandoff {
    pub(crate) fn instrumentation(&self) -> Option<InstrumentationScope> {
        self.parts
            .as_ref()
            .and_then(|parts| parts.request.instrumentation())
            .map(|captured| {
                captured.handoff_scope(InstrumentationKind::Provider {
                    phase: chigiri_runtime::LifecyclePhase::TransportAdmission,
                    component: "websocket-opening",
                })
            })
    }

    pub(crate) fn activity(&self) -> crate::managed::EventActivity {
        self.activity.clone()
    }

    pub(crate) fn new(
        parts: ClaimedConnectionParts,
        deadlines: &'static dyn DeadlineFactory,
        authorizer: Arc<dyn ConnectionInitAuthorizer>,
        observer: Arc<dyn AuthorizationErrorObserver>,
        activity: crate::managed::EventActivity,
    ) -> Self {
        let activity = parts.connection.socket_activity().unwrap_or(activity);
        let instrumentation = parts.request.instrumentation().map(|captured| {
            captured.handoff_scope(InstrumentationKind::Provider {
                phase: chigiri_runtime::LifecyclePhase::TransportAdmission,
                component: "websocket-initialization-deadline",
            })
        });
        let initialization_timer = match instrumentation {
            Some(scope) => {
                let timer = scope.run(|| deadlines.sleep(INITIALIZATION_TIMEOUT));
                Box::pin(scope.instrument(timer)) as chigiri_http::BoxFuture<'static, ()>
            }
            None => deadlines.sleep(INITIALIZATION_TIMEOUT),
        };
        Self {
            parts: Some(parts),
            initialization_timer: Some(initialization_timer),
            deadlines,
            authorizer,
            observer,
            activity,
        }
    }

    fn close_parts(&mut self) -> WebSocketConnectionCleanup {
        let parts = self.parts.take().expect("live WebSocket handoff");
        let initialization_timer = self
            .initialization_timer
            .take()
            .expect("live initialization timer");
        cleanup::framework(
            parts.connection,
            cleanup::disposal::ConnectionDisposal {
                _timers: [Some(parts.deadline), Some(initialization_timer)],
                ..cleanup::disposal::ConnectionDisposal::metadata(
                    Arc::new(parts.request),
                    parts.owner,
                )
            },
            WebSocketConnectionClose::TIMED_OUT,
            self.deadlines,
        )
    }
}

impl Future for WebSocketConnectionHandoff {
    type Output = WebSocketConnectionHandoffResolution;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let deadline = &mut self
            .parts
            .as_mut()
            .expect("live WebSocket handoff")
            .deadline;
        if deadline.as_mut().poll(context).is_ready() {
            return Poll::Ready(WebSocketConnectionHandoffResolution::Closing(
                self.close_parts(),
            ));
        }
        if self
            .initialization_timer
            .as_mut()
            .expect("live initialization timer")
            .as_mut()
            .poll(context)
            .is_ready()
        {
            return Poll::Ready(WebSocketConnectionHandoffResolution::Closing(
                self.close_parts(),
            ));
        }
        let parts = self.parts.take().expect("live WebSocket handoff");
        drop(parts.deadline);
        let timer = self
            .initialization_timer
            .take()
            .expect("live initialization timer");
        let controls = WebSocketControlStaging::new(parts.owner.clone());
        Poll::Ready(WebSocketConnectionHandoffResolution::Initializing(
            Box::new(WebSocketConnectionInitialization::new(
                parts.connection,
                Arc::new(parts.request),
                parts.owner,
                controls,
                timer,
                self.deadlines,
                Arc::clone(&self.authorizer),
                Arc::clone(&self.observer),
                self.activity.clone(),
            )),
        ))
    }
}

impl Drop for WebSocketConnectionHandoff {
    fn drop(&mut self) {
        let Some(parts) = self.parts.take() else {
            return;
        };
        let initialization_timer = self
            .initialization_timer
            .take()
            .expect("live initialization timer");
        drop(cleanup::framework(
            parts.connection,
            cleanup::disposal::ConnectionDisposal {
                _timers: [Some(parts.deadline), Some(initialization_timer)],
                ..cleanup::disposal::ConnectionDisposal::metadata(
                    Arc::new(parts.request),
                    parts.owner,
                )
            },
            WebSocketConnectionClose::INTERNAL,
            self.deadlines,
        ));
    }
}
