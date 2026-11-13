//! Test-only faults and barriers; none are part of the bridge integration SPI.

use std::sync::Arc;
use std::task::Waker;

use super::HttpStreamingDriver;
use crate::transport::streaming::supervisor::body::BodyPort;

impl HttpStreamingDriver {
    pub(crate) fn registry_lock_probe_for_test(
        &self,
    ) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        let registry = Arc::clone(&self.registry);
        Arc::new(move || match registry.state.try_lock() {
            Ok(state) => {
                drop(state);
                false
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                drop(poisoned.into_inner());
                false
            }
            Err(std::sync::TryLockError::WouldBlock) => true,
        })
    }

    pub(crate) fn set_registry_waker_for_test(&self, waker: Waker) {
        let previous = {
            let mut state = self
                .registry
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.driver.replace(waker)
        };
        drop(previous);
    }

    pub(crate) fn registry_ownership_for_test(&self) -> (usize, usize) {
        let pending = self
            .registry
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .slots
            .iter()
            .filter(|slot| slot.pending.is_some())
            .count();
        let active = self.entries.iter().filter(|entry| entry.is_some()).count();
        (pending, active)
    }

    pub(crate) fn inject_registry_conflict_for_test(&mut self) {
        let active_index = self
            .entries
            .iter()
            .position(Option::is_some)
            .expect("one active registry test entry");
        let donor_index = self
            .entries
            .iter()
            .enumerate()
            .find_map(|(index, entry)| (index != active_index && entry.is_some()).then_some(index))
            .expect("a distinct registry test donor");
        let donor = self.entries[donor_index]
            .take()
            .expect("located registry test donor");
        let previous = {
            let mut state = self
                .registry
                .state
                .lock()
                .expect("unpoisoned registry conflict fixture");
            state.slots[active_index].pending.replace(donor)
        };
        assert!(previous.is_none(), "empty conflict target slot");
    }

    pub(crate) fn set_clock_for_test(
        &mut self,
        absolute_nanos: u64,
    ) -> Result<(), super::HttpStreamingError> {
        self.clock.set_for_test(absolute_nanos)?;
        self.timer.take();
        Ok(())
    }

    pub(crate) fn before_close_for_test(&self, barrier: impl FnOnce() + Send + 'static) {
        self.test_port().before_close_for_test(Box::new(barrier));
    }

    pub(crate) fn panic_on_encode_for_test(
        &self,
        terminal: bool,
    ) -> Arc<std::sync::atomic::AtomicUsize> {
        self.test_port().panic_on_encode_for_test(terminal)
    }

    pub(crate) fn poison_registry_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _state = self
                .registry
                .state
                .lock()
                .expect("unpoisoned test registry");
            panic!("injected registry poison");
        }));
    }

    pub(crate) fn before_offer_for_test(&self, barrier: impl FnOnce() + Send + 'static) {
        self.test_port().before_offer_for_test(Box::new(barrier));
    }

    pub(crate) fn before_demand_for_test(&self, barrier: impl FnOnce() + Send + 'static) {
        self.test_port().before_demand_for_test(Box::new(barrier));
    }

    pub(crate) fn after_demand_check_for_test(&self, barrier: impl FnOnce() + Send + 'static) {
        self.test_port()
            .after_demand_check_for_test(Box::new(barrier));
    }

    fn test_port(&self) -> Arc<BodyPort> {
        if let Some(entry) = self.entries.iter().flatten().next() {
            return Arc::clone(entry.test_port());
        }
        let state = self
            .registry
            .state
            .lock()
            .expect("unpoisoned test registry");
        Arc::clone(
            state
                .slots
                .iter()
                .find_map(|slot| slot.pending.as_ref())
                .expect("attached test stream")
                .test_port(),
        )
    }
}
