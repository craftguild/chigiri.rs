//! One-shot barriers for exact close-versus-handoff transition fixtures.

use super::BodyPort;

type Barrier = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub(super) struct Hooks {
    before_offer: Option<Barrier>,
    before_demand: Option<Barrier>,
    after_demand_check: Option<Barrier>,
    before_close: Option<Barrier>,
    before_worker: Option<Barrier>,
    encoding: Option<EncodingFault>,
}

struct EncodingFault {
    terminal: bool,
    armed: bool,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl BodyPort {
    pub(in crate::transport::streaming::supervisor) fn run_before_worker_for_test(&self) {
        let barrier = self.faults.lock().unwrap().before_worker.take();
        if let Some(barrier) = barrier {
            barrier();
        }
    }

    pub(in crate::transport::streaming::supervisor) fn before_close_for_test(
        &self,
        barrier: Barrier,
    ) {
        self.faults.lock().unwrap().before_close = Some(barrier);
    }

    pub(in crate::transport::streaming::supervisor) fn run_before_close_for_test(&self) {
        let barrier = self.faults.lock().unwrap().before_close.take();
        if let Some(barrier) = barrier {
            barrier();
        }
    }

    pub(in crate::transport::streaming::supervisor) fn panic_on_encode_for_test(
        &self,
        terminal: bool,
    ) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.faults.lock().unwrap().encoding = Some(EncodingFault {
            terminal,
            armed: true,
            dropped: std::sync::Arc::clone(&dropped),
        });
        dropped
    }

    pub(in crate::transport::streaming::supervisor) fn payload_probe_for_test(
        &self,
        terminal: bool,
    ) -> Option<std::sync::Arc<std::sync::atomic::AtomicUsize>> {
        self.faults
            .lock()
            .unwrap()
            .encoding
            .as_ref()
            .filter(|fault| fault.terminal == terminal)
            .map(|fault| std::sync::Arc::clone(&fault.dropped))
    }

    pub(in crate::transport::streaming::supervisor) fn run_encode_fault_for_test(
        &self,
        terminal: bool,
    ) {
        let panic = {
            let mut faults = self.faults.lock().unwrap();
            faults.encoding.as_mut().is_some_and(|fault| {
                if fault.armed && fault.terminal == terminal {
                    fault.armed = false;
                    true
                } else {
                    false
                }
            })
        };
        if panic {
            panic!("injected bounded HTTP encoding unwind");
        }
    }

    pub(in crate::transport::streaming::supervisor) fn before_offer_for_test(
        &self,
        barrier: Barrier,
    ) {
        self.faults.lock().unwrap().before_offer = Some(barrier);
    }

    pub(in crate::transport::streaming::supervisor) fn before_demand_for_test(
        &self,
        barrier: Barrier,
    ) {
        self.faults.lock().unwrap().before_demand = Some(barrier);
    }

    pub(in crate::transport::streaming::supervisor) fn after_demand_check_for_test(
        &self,
        barrier: Barrier,
    ) {
        self.faults.lock().unwrap().after_demand_check = Some(barrier);
    }

    pub(in crate::transport::streaming::supervisor) fn run_before_offer_for_test(&self) {
        let barrier = self.faults.lock().unwrap().before_offer.take();
        if let Some(barrier) = barrier {
            barrier();
        }
    }

    pub(in crate::transport::streaming::supervisor) fn run_before_demand_for_test(&self) {
        let barrier = self.faults.lock().unwrap().before_demand.take();
        if let Some(barrier) = barrier {
            barrier();
        }
    }

    pub(in crate::transport::streaming::supervisor) fn run_after_demand_check_for_test(&self) {
        let barrier = self.faults.lock().unwrap().after_demand_check.take();
        if let Some(barrier) = barrier {
            barrier();
        }
    }
}

impl super::HttpStreamingBody {
    pub(crate) fn before_worker_for_test(&self, barrier: Box<dyn FnOnce() + Send>) {
        self.port.faults.lock().unwrap().before_worker = Some(barrier);
    }
}
