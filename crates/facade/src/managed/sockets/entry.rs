//! One closed SDK upgrade role, followed by the existing protocol session.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::{ManagedServiceError, SocketWork};
use crate::__streaming::{
    ClaimedWebSocketUpgrade, WebSocketIo, WebSocketIoError, WebSocketSessionDriver,
};
use crate::managed::{EventActivity, HttpFaultReporter, StopPhase};
use crate::subscription::worker::{
    ConcreteWorkerOutcome, SubscriptionWorkerRuntimeError, websocket::NativeUpgradeWorker,
};
use crate::transport::EndpointState;
use crate::{BoxFuture, DeadlineFactory};

pub(super) struct Entry<C: Send + Sync + 'static, S: WebSocketIo> {
    endpoint: Option<Arc<EndpointState<C>>>,
    state: EntryState<C, S>,
    deadlines: &'static dyn DeadlineFactory,
    stopping: bool,
    scope: Arc<super::SocketScope>,
}

enum EntryState<C: Send + Sync + 'static, S: WebSocketIo> {
    Upgrade(Box<Upgrade<S>>),
    Session(WebSocketSessionDriver<C, super::io::GuardedSocket<S>>),
    Done,
}

struct Upgrade<S: WebSocketIo> {
    seed: Option<ClaimedWebSocketUpgrade<'static>>,
    worker: Option<NativeUpgradeWorker<S>>,
}

impl<S: WebSocketIo> Drop for Upgrade<S> {
    fn drop(&mut self) {
        // Closing the bounded result handoff precedes retiring its original
        // worker. The same connection owner remains live throughout disposal.
        let completion = self.worker.take().and_then(NativeUpgradeWorker::retire);
        if let (Some(seed), Some(completion)) = (self.seed.take(), completion) {
            seed.retire_native_upgrade(completion);
        }
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> Entry<C, S> {
    pub(super) fn new(
        endpoint: Arc<EndpointState<C>>,
        mut seed: ClaimedWebSocketUpgrade<'static>,
        socket: BoxFuture<'static, Result<S, WebSocketIoError>>,
        deadlines: &'static dyn DeadlineFactory,
        scope: Arc<super::SocketScope>,
    ) -> Result<Self, ManagedServiceError> {
        seed.bind_native_scope(&scope);
        let activity = EventActivity::new(endpoint.subscription_execution.managed.get().cloned());
        let reporter = HttpFaultReporter::authorization(crate::transport::websocket::observer(
            endpoint.schema.clone(),
            seed.request_telemetry().clone(),
        ));
        let worker = seed
            .native_worker(socket, activity, reporter)
            .map_err(|_| ManagedServiceError::Infrastructure)?;
        Ok(Self {
            endpoint: Some(endpoint),
            state: EntryState::Upgrade(Box::new(Upgrade {
                seed: Some(seed),
                worker: Some(worker),
            })),
            deadlines,
            stopping: false,
            scope,
        })
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> SocketWork for Entry<C, S> {
    fn poll(
        &mut self,
        context: &mut Context<'_>,
        _phase: StopPhase,
    ) -> Poll<Result<(), ManagedServiceError>> {
        let phase = self
            .scope
            .activity()
            .control()
            .map_or(StopPhase::Cancelled, |control| control.phase());
        if phase != StopPhase::Running && !self.stopping {
            self.stopping = true;
            match &mut self.state {
                EntryState::Session(session) => match phase {
                    StopPhase::Graceful { .. } => session.graceful(),
                    _ => session.shutdown(),
                },
                _ => {
                    self.state = EntryState::Done;
                    self.endpoint.take();
                }
            }
        }
        loop {
            match &mut self.state {
                EntryState::Upgrade(upgrade) => {
                    // Keep the original pre-101 deadline ahead of native upgrade
                    // and wrapper polling. No timeout is restarted at handoff.
                    if upgrade
                        .seed
                        .as_mut()
                        .expect("live upgrade seed")
                        .poll_upgrade_deadline(context)
                        .is_ready()
                    {
                        self.state = EntryState::Done;
                        self.endpoint.take();
                        continue;
                    }
                    let socket =
                        match Pin::new(upgrade.worker.as_mut().expect("live upgrade worker"))
                            .poll(context)
                        {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(ConcreteWorkerOutcome::Completed(
                                Ok(Err(_)) | Err(SubscriptionWorkerRuntimeError::EndpointClosed),
                            )) => {
                                self.state = EntryState::Done;
                                self.endpoint.take();
                                continue;
                            }
                            Poll::Ready(ConcreteWorkerOutcome::Completed(Ok(Ok(socket)))) => socket,
                            Poll::Ready(_) => {
                                return Poll::Ready(Err(ManagedServiceError::Infrastructure));
                            }
                        };
                    let seed = upgrade.seed.take().expect("native conversion seed");
                    let socket = super::io::GuardedSocket::new(socket, Arc::clone(&self.scope));
                    if self.scope.activity().is_stopping() {
                        drop(socket);
                        drop(seed);
                        self.state = EntryState::Done;
                        self.endpoint.take();
                        continue;
                    }
                    let endpoint = self.endpoint.take().expect("managed upgrade endpoint");
                    let session = endpoint
                        .websocket_session(seed, socket, self.deadlines)
                        .map_err(|_| ManagedServiceError::Infrastructure)?;
                    self.state = EntryState::Session(session);
                }
                EntryState::Session(session) => {
                    if Pin::new(session).poll(context).is_pending() {
                        return Poll::Pending;
                    }
                    self.state = EntryState::Done;
                }
                EntryState::Done => return Poll::Ready(Ok(())),
            }
        }
    }
}
