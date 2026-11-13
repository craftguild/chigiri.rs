//! One application-polled owner from upgraded opening through final cleanup.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_http::HttpLifecycle;
use chigiri_runtime::instrumentation::InstrumentationScope;

use super::active::{ActiveConfiguration, ActiveConnection};
use super::{WebSocketIo, WebSocketOpeningDriver, WebSocketOpeningOutcome};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::execution_state::SubscriptionExecutionState;

enum Session<C: Send + Sync + 'static, S: WebSocketIo> {
    Opening(Box<WebSocketOpeningDriver<S>>),
    Active(Box<ActiveConnection<C, S>>),
    Closed,
}

/// Drives one acknowledged multiplexed socket without spawning a task.
///
/// The application must poll this future inside its explicitly owned runtime,
/// and keep its endpoint runtime guard until all connection drivers finish.
/// Drop cancels logical operations and transfers unfinished work to its existing
/// charged quarantine. No listener, router, connection task or runtime is owned
/// or created by this driver.
#[must_use = "the connection must be polled or cancelled by dropping its driver"]
#[doc(hidden)]
pub struct WebSocketSessionDriver<C: Send + Sync + 'static, S: WebSocketIo> {
    session: Session<C, S>,
    configuration: Option<ActiveConfiguration<C>>,
    instrumentation: Option<InstrumentationScope>,
}

impl<C: Send + Sync + 'static, S: WebSocketIo> WebSocketSessionDriver<C, S> {
    pub(crate) fn graceful(&mut self) {
        self.with_context(|this| match &mut this.session {
            Session::Active(active) => active.graceful(),
            Session::Opening(_) => this.session = Session::Closed,
            Session::Closed => {}
        });
    }

    /// Selects explicit application shutdown before physical connection cleanup.
    ///
    /// Continue polling this driver under the application's existing shutdown
    /// deadline. A later Drop or broken socket cannot replace this decision.
    pub fn shutdown(&mut self) {
        self.with_context(|this| match &mut this.session {
            Session::Active(active) => active.shutdown(),
            Session::Opening(_) => this.session = Session::Closed,
            Session::Closed => {}
        });
    }

    /// Selects a known peer/connection transport failure before owner Drop.
    /// This does not classify an unexplained abandoned driver as peer failure.
    pub fn transport_closed(&mut self) {
        self.with_context(|this| match &mut this.session {
            Session::Active(active) => active.transport_closed(),
            Session::Opening(_) => this.session = Session::Closed,
            Session::Closed => {}
        });
    }

    pub(crate) fn new(
        opening: WebSocketOpeningDriver<S>,
        execution: Arc<SubscriptionExecutionState<C>>,
        lifecycle: HttpLifecycle,
        owner: EndpointByteOwner,
    ) -> Self {
        let instrumentation = opening.instrumentation();
        Self {
            session: Session::Opening(Box::new(opening)),
            configuration: Some(ActiveConfiguration {
                execution,
                lifecycle,
                owner,
            }),
            instrumentation,
        }
    }

    fn with_context<R>(&mut self, callback: impl FnOnce(&mut Self) -> R) -> R {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| callback(self)),
            None => callback(self),
        }
    }

    #[cfg(test)]
    pub(crate) fn setup_finished_for_test(&self, id: &str) -> bool {
        matches!(&self.session, Session::Active(active) if active.setup_finished_for_test(id))
    }

    #[cfg(test)]
    pub(crate) fn bind_managed_for_test(&self, control: &Arc<crate::managed::Control>) {
        match &self.session {
            Session::Active(active) => active.bind_managed_for_test(control),
            _ => panic!("test requires an active connection"),
        }
    }

    #[cfg(test)]
    pub(crate) fn before_send_for_test(&mut self, action: Box<dyn FnOnce() + Send>) {
        match &mut self.session {
            Session::Active(active) => active.set_before_send_for_test(action),
            _ => panic!("test requires an active connection"),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_clock_for_test(&mut self) {
        if let Session::Active(active) = &mut self.session {
            active.fail_clock_for_test();
        }
    }

    #[cfg(test)]
    pub(crate) fn operation_state_for_test(
        &self,
        id: &str,
    ) -> Option<(
        crate::subscription::bridge_resources::BridgeGeneration,
        bool,
        bool,
    )> {
        match &self.session {
            Session::Active(active) => active.operation_state_for_test(id),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn queued_priority_for_test(&self) -> bool {
        matches!(&self.session, Session::Active(active) if active.queued_priority_for_test())
    }

    #[cfg(test)]
    pub(crate) fn set_time_for_test(&mut self, now: u64) {
        if let Session::Active(active) = &mut self.session {
            active.set_time_for_test(now);
        }
    }

    #[cfg(test)]
    pub(crate) fn encoded_bytes_for_test(&self) -> usize {
        match &self.session {
            Session::Active(active) => active.encoded_bytes_for_test(),
            _ => 0,
        }
    }
    fn poll_owned(&mut self, context: &mut Context<'_>) -> Poll<()> {
        loop {
            match &mut self.session {
                Session::Opening(opening) => match Pin::new(opening).poll(context) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(WebSocketOpeningOutcome::Closed) => {
                        self.session = Session::Closed;
                    }
                    Poll::Ready(WebSocketOpeningOutcome::Acknowledged { socket, connection }) => {
                        let configuration =
                            self.configuration.take().expect("session configuration");
                        self.session = match connection.into_session_parts(&configuration.owner) {
                            Ok(parts) => {
                                match ActiveConnection::new(configuration, socket, parts) {
                                    Ok(active) => Session::Active(Box::new(active)),
                                    Err(_) => Session::Closed,
                                }
                            }
                            Err(_) => Session::Closed,
                        };
                    }
                },
                Session::Active(active) => return Pin::new(active).poll(context),
                Session::Closed => return Poll::Ready(()),
            }
        }
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> Future for WebSocketSessionDriver<C, S> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        self.get_mut().with_context(|this| this.poll_owned(context))
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> Drop for WebSocketSessionDriver<C, S> {
    fn drop(&mut self) {
        let owned = (
            std::mem::replace(&mut self.session, Session::Closed),
            self.configuration.take(),
        );
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| drop(owned)),
            None => drop(owned),
        }
    }
}
