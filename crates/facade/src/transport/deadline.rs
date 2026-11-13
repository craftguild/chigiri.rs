//! Explicit HTTP phase clocks; the neutral path remains runtime independent.

#[cfg(feature = "subscription-worker")]
mod managed;
#[cfg(feature = "subscription-worker")]
pub(crate) use managed::HttpDeadlineState;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};
use chigiri_runtime::LifecyclePhase;

use super::TimeoutKind;

#[derive(Clone, Default)]
pub(crate) struct HttpDeadlineMonitor {
    #[cfg(feature = "subscription-worker")]
    managed: Option<managed::ManagedHttpClock>,
}

impl HttpDeadlineMonitor {
    pub(crate) fn admit_finite(&self) -> bool {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            return clock.admit_finite();
        }
        true
    }

    pub(crate) fn poison(&self) {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            clock.poison();
        }
    }
    pub(crate) fn is_managed(&self) -> bool {
        #[cfg(feature = "subscription-worker")]
        {
            self.managed.is_some()
        }
        #[cfg(not(feature = "subscription-worker"))]
        {
            false
        }
    }

    pub(crate) fn completion_outcome(&self) -> Option<chigiri_http::ResponseCompletionOutcome> {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            return Some(clock.completion_outcome());
        }
        None
    }

    pub(crate) fn fail(&self) {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            clock.fail();
        }
    }

    pub(crate) fn timed_out(&self) {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            clock.timed_out();
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn faulted(&self) -> bool {
        self.managed
            .as_ref()
            .is_some_and(managed::ManagedHttpClock::faulted)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) async fn registered(&self) -> bool {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            return clock.registered().await;
        }
        true
    }

    pub(crate) fn overall<'a>(
        &self,
        factory: &'a dyn DeadlineFactory,
        timeout: Duration,
    ) -> BoxFuture<'a, ()> {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            return clock.overall();
        }
        factory.sleep(timeout)
    }

    pub(crate) async fn run<F, T>(
        &self,
        phase: LifecyclePhase,
        factory: &dyn DeadlineFactory,
        timeout: Duration,
        overall: Pin<&mut (dyn Future<Output = ()> + Send)>,
        create: impl FnOnce() -> F + Send,
    ) -> Result<T, TimeoutKind>
    where
        F: Future<Output = T> + Send,
    {
        #[cfg(feature = "subscription-worker")]
        if let Some(clock) = &self.managed {
            return clock.run(phase, timeout, overall, create).await;
        }
        let _ = phase;
        super::run(factory, timeout, overall, create()).await
    }
}
