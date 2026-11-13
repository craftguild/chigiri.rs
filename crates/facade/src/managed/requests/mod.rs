//! Closed HTTP worker ownership, separate from subscription registrations.

mod driver;
mod entry;
mod registry;
mod response;
mod task;
#[cfg(test)]
mod tests;

pub(crate) use driver::RequestDriver;
pub(crate) use entry::RetainedHttpRequest;
pub(crate) use registry::RequestRegistry;
use response::RequestResponse;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Control;

pub(super) struct Disconnect {
    closed: AtomicBool,
    control: Arc<Control>,
}

impl Disconnect {
    fn new(control: Arc<Control>) -> Arc<Self> {
        Arc::new(Self {
            closed: AtomicBool::new(false),
            control,
        })
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.control.notify();
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}
