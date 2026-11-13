//! Tokio timer integration for the transport-neutral deadline SPI.

use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

pub(crate) struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        let deadline = tokio::time::Instant::now().checked_add(duration);
        Box::pin(async move {
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Poll, Waker};

    use super::*;

    #[test]
    fn unschedulable_deadline_completes_immediately() {
        let mut future = TokioDeadline.sleep(Duration::MAX);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(()));
    }
}
