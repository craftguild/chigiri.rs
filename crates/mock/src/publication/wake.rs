//! One fixed internal wake target for the Worker's joined delivery driver.

use std::sync::OnceLock;
use std::thread::Thread;

#[derive(Default)]
pub(crate) struct DeliveryWake {
    driver: OnceLock<Thread>,
}

impl DeliveryWake {
    pub(crate) fn install(&self) -> Result<(), ()> {
        self.driver.set(std::thread::current()).map_err(|_| ())
    }

    pub(crate) fn notify(&self) {
        if let Some(driver) = self.driver.get() {
            driver.unpark();
        }
    }
}
