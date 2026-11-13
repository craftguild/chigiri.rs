//! One concrete, owned SDK supervisor; no application task or future registry.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::watch;

use super::requests::RequestDriver;
use super::sockets::SocketDriver;
use super::{Control, StopPhase};
use crate::__streaming::{SubscriptionRuntimeGuard, SubscriptionRuntimeShutdown};
use crate::transport::streaming::supervisor::{
    HttpStreamingDriver, HttpStreamingError, HttpStreamingRegistration,
};

pub(crate) struct Supervisor {
    control: Arc<Control>,
    registration: HttpStreamingRegistration,
    driver: Option<HttpStreamingDriver>,
    socket_driver: Option<SocketDriver>,
    sockets: Arc<super::SocketRegistry>,
    request_driver: Option<RequestDriver>,
    guard: Option<SubscriptionRuntimeGuard>,
    cleanup: Option<SubscriptionRuntimeShutdown>,
    changed: watch::Receiver<()>,
    notifications: Option<crate::BoxFuture<'static, watch::Receiver<()>>>,
    completed: bool,
    deadline: Option<(tokio::time::Instant, Pin<Box<tokio::time::Sleep>>)>,
}

impl Supervisor {
    pub(crate) fn new(
        control: Arc<Control>,
        registration: HttpStreamingRegistration,
        driver: HttpStreamingDriver,
        socket_driver: SocketDriver,
        request_driver: RequestDriver,
        guard: SubscriptionRuntimeGuard,
    ) -> Self {
        let changed = control.subscribe();
        let sockets = socket_driver.registry();
        Self {
            control,
            registration,
            driver: Some(driver),
            socket_driver: Some(socket_driver),
            sockets,
            request_driver: Some(request_driver),
            guard: Some(guard),
            cleanup: None,
            changed,
            notifications: None,
            completed: false,
            deadline: None,
        }
    }

    pub(crate) fn control(&self) -> Arc<Control> {
        Arc::clone(&self.control)
    }

    pub(crate) fn sockets(&self) -> Arc<super::SocketRegistry> {
        Arc::clone(&self.sockets)
    }

    fn poll_notification(&mut self, context: &mut Context<'_>) {
        if self.notifications.is_none() {
            let mut receiver = self.changed.clone();
            self.notifications = Some(Box::pin(async move {
                let _ = receiver.changed().await;
                receiver
            }));
        }
        if let Poll::Ready(receiver) = self
            .notifications
            .as_mut()
            .expect("notification")
            .as_mut()
            .poll(context)
        {
            self.changed = receiver;
            self.notifications.take();
            context.waker().wake_by_ref();
        }
    }
}

impl Future for Supervisor {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        self.poll_notification(context);
        if let StopPhase::Graceful { deadline } = self.control.phase() {
            if self
                .deadline
                .as_ref()
                .is_none_or(|(old, _)| *old != deadline)
            {
                self.deadline = Some((deadline, Box::pin(tokio::time::sleep_until(deadline))));
            }
            if self
                .deadline
                .as_mut()
                .expect("scoped deadline")
                .1
                .as_mut()
                .poll(context)
                .is_ready()
            {
                self.control.expire(tokio::time::Instant::now());
            }
        }
        if self.control.outcome() == Some(crate::ShutdownOutcome::Expired) {
            // No fresh five-second wait after expiry. Dropping these owners
            // immediately hands unjoined work to existing charged recovery.
            if let Some(driver) = &mut self.driver {
                driver.fail(HttpStreamingError::TimedOut);
            }
            self.driver.take();
            self.socket_driver.take();
            self.request_driver.take();
            self.guard.take();
            self.cleanup.take();
            self.completed = true;
            return Poll::Ready(());
        }
        if let Some(driver) = &mut self.request_driver
            && let Poll::Ready(result) = driver.poll(context)
        {
            if result.is_err() {
                self.control.fail();
            }
            self.request_driver.take();
        }
        if let StopPhase::Graceful { .. } = self.control.phase()
            && let Some(driver) = &mut self.driver
        {
            driver.graceful();
        }
        if self.control.is_stopping() {
            self.registration.close();
        }
        if let Some(driver) = &mut self.driver
            && let Poll::Ready(result) = Pin::new(driver).poll(context)
        {
            if result.is_err() {
                self.control.fail();
            }
            self.driver.take();
        }
        let phase = self.control.phase();
        if self.control.outcome() == Some(crate::ShutdownOutcome::InfrastructureFailure) {
            self.socket_driver.take();
        } else if let Some(driver) = &mut self.socket_driver
            && let Poll::Ready(result) = driver.poll(context, phase)
        {
            if result.is_err() {
                self.control.fail();
            }
            self.socket_driver.take();
        }
        if self.sockets.has_poisoned() {
            self.control.fail();
            self.driver.take();
            self.socket_driver.take();
            self.request_driver.take();
            self.guard.take();
            self.cleanup.take();
            self.completed = true;
            return Poll::Ready(());
        }
        if self.driver.is_some() || self.socket_driver.is_some() || self.request_driver.is_some() {
            return Poll::Pending;
        }
        if let Some(guard) = self.guard.take() {
            match guard.shutdown() {
                Ok(cleanup) => self.cleanup = Some(cleanup),
                Err(_) => self.control.fail(),
            }
        }
        if let Some(cleanup) = &mut self.cleanup {
            match Pin::new(cleanup).poll(context) {
                Poll::Ready(result) => {
                    if result.is_err() {
                        self.control.fail();
                    }
                    self.cleanup.take();
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
        if !self.control.requests_idle() {
            return Poll::Pending;
        }
        self.control.drained(tokio::time::Instant::now());
        self.completed = true;
        Poll::Ready(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if !self.completed {
            self.control.fail();
            if let Some(driver) = &mut self.driver {
                driver.fail(HttpStreamingError::Worker);
            }
            self.registration.close();
        }
        // Driver destruction transfers every unfinished concrete worker and
        // charge through the existing generation-keyed cleanup owners.
        drop(self.driver.take());
        drop(self.socket_driver.take());
        drop(self.request_driver.take());
        drop(self.guard.take());
    }
}
