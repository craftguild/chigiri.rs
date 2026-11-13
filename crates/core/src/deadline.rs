//! Minimal SPI for injecting runtime timers into request execution.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Object-safe contract for supplying runtime timer futures.
pub trait DeadlineFactory: Send + Sync + 'static {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}
