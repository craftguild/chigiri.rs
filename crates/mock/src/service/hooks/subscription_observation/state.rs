//! Linearize dispatch entry, registration, and outside terminal selection in one fixed state.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use chigiri::__private::{SubscriptionObservationReceipt, SubscriptionTermination};
use chigiri::SubscriptionOverflow;

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;

pub(super) const fn allocation_bytes() -> usize {
    std::mem::size_of::<Shared>() + 2 * std::mem::size_of::<usize>()
}

pub(super) struct Shared {
    receipt: SubscriptionObservationReceipt,
    state: Mutex<State>,
    source: OnceLock<Arc<chigiri::__private::HostSourceDelivery>>,
}

pub(in crate::service::hooks) struct EntryFence {
    shared: Arc<Shared>,
}

pub(in crate::service) struct RegistrationFence {
    shared: Arc<Shared>,
}

pub(in crate::service) struct RegisteredSignal {
    shared: Weak<Shared>,
    receipt: SubscriptionObservationReceipt,
}

#[derive(Clone, Copy)]
pub(super) enum Record {
    SetupRegistered {
        generation: u64,
        overflow: SubscriptionOverflow,
    },
    SetupUnregistered {
        cancelled: bool,
    },
    Terminal {
        generation: u64,
        decision: SubscriptionTermination,
        dropped_total: u64,
    },
}

pub(super) struct Drain {
    pub(super) first: Option<Record>,
    pub(super) second: Option<Record>,
}

#[derive(Clone, Copy)]
enum Phase {
    AwaitingEntry,
    Entered,
    Registered {
        generation: u64,
        overflow: SubscriptionOverflow,
    },
}

struct State {
    phase: Phase,
    decision: Option<SubscriptionTermination>,
    setup_emitted: bool,
    terminal_emitted: bool,
    last_drop: u64,
}

impl Shared {
    pub(super) fn new(receipt: SubscriptionObservationReceipt) -> Arc<Self> {
        Arc::new(Self {
            receipt,
            state: Mutex::new(State::new()),
            source: OnceLock::new(),
        })
    }

    pub(super) fn entry(self: &Arc<Self>) -> EntryFence {
        EntryFence {
            shared: Arc::clone(self),
        }
    }

    pub(super) fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        if self.receipt.is_closed() {
            return false;
        }
        self.state
            .lock()
            .is_ok_and(|state| state.registered(overflow))
    }

    pub(super) fn finish(&self, decision: SubscriptionTermination) {
        if let Ok(mut state) = self.state.lock() {
            state.finish(decision);
        }
    }

    pub(super) fn drop_oldest(&self, total: u64) -> Option<u64> {
        self.state.lock().ok()?.drop_oldest(total)
    }

    pub(super) fn drain(&self, dropped_total: u64) -> Drain {
        let Ok(mut state) = self.state.lock() else {
            return Drain {
                first: None,
                second: None,
            };
        };
        state.drain(dropped_total)
    }
}

impl State {
    const fn new() -> Self {
        Self {
            phase: Phase::AwaitingEntry,
            decision: None,
            setup_emitted: false,
            terminal_emitted: false,
            last_drop: 0,
        }
    }

    fn enter(&mut self) -> bool {
        if self.decision.is_some() || !matches!(self.phase, Phase::AwaitingEntry) {
            return false;
        }
        self.phase = Phase::Entered;
        true
    }

    fn register(&mut self, generation: u64, overflow: SubscriptionOverflow) -> bool {
        if self.decision.is_some() || !matches!(self.phase, Phase::Entered) {
            return false;
        }
        self.phase = Phase::Registered {
            generation,
            overflow,
        };
        true
    }

    fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        self.decision.is_none()
            && matches!(
                self.phase,
                Phase::Registered {
                    overflow: selected,
                    ..
                } if selected == overflow
            )
    }

    fn finish(&mut self, decision: SubscriptionTermination) {
        if self.decision.is_none() {
            self.decision = Some(decision);
        }
    }

    fn drop_oldest(&mut self, total: u64) -> Option<u64> {
        let Phase::Registered { generation, .. } = self.phase else {
            return None;
        };
        if self.decision.is_some() || total == 0 || total <= self.last_drop {
            return None;
        }
        self.last_drop = total;
        Some(generation)
    }

    fn drain(&mut self, dropped_total: u64) -> Drain {
        let mut first = None;
        let mut second = None;
        if !self.setup_emitted {
            match (self.phase, self.decision) {
                (
                    Phase::Registered {
                        generation,
                        overflow,
                    },
                    _,
                ) => {
                    first = Some(Record::SetupRegistered {
                        generation,
                        overflow,
                    });
                    self.setup_emitted = true;
                }
                (Phase::Entered, Some(decision)) => {
                    first = Some(Record::SetupUnregistered {
                        cancelled: decision.is_cancellation(),
                    });
                    self.setup_emitted = true;
                    self.terminal_emitted = true;
                }
                (Phase::AwaitingEntry, Some(_)) => {
                    self.setup_emitted = true;
                    self.terminal_emitted = true;
                }
                _ => {}
            }
        }
        if !self.terminal_emitted
            && let (Phase::Registered { generation, .. }, Some(decision)) =
                (self.phase, self.decision)
        {
            let record = Record::Terminal {
                generation,
                decision,
                dropped_total,
            };
            if first.is_some() {
                second = Some(record);
            } else {
                first = Some(record);
            }
            self.terminal_emitted = true;
        }
        Drain { first, second }
    }
}

impl EntryFence {
    pub(in crate::service) fn enter(&self) -> Result<RegistrationFence, ()> {
        if self.shared.receipt.is_closed() {
            return Err(());
        }
        let mut state = self.shared.state.lock().map_err(|_| ())?;
        if self.shared.receipt.is_closed() || !state.enter() {
            return Err(());
        }
        Ok(RegistrationFence {
            shared: Arc::clone(&self.shared),
        })
    }
}

impl RegistrationFence {
    pub(in crate::service) fn registered(
        &self,
        generation: u64,
        overflow: SubscriptionOverflow,
        source: Arc<chigiri::__private::HostSourceDelivery>,
    ) -> Result<RegisteredSignal, ()> {
        if generation == 0 || self.shared.receipt.is_closed() {
            return Err(());
        }
        let mut state = self.shared.state.lock().map_err(|_| ())?;
        if self.shared.receipt.is_closed() || !state.register(generation, overflow) {
            return Err(());
        }
        self.shared.source.set(source).map_err(|_| ())?;
        let signal = RegisteredSignal {
            shared: Arc::downgrade(&self.shared),
            receipt: self.shared.receipt.clone(),
        };
        Ok(signal)
    }
}

impl RegisteredSignal {
    pub(in crate::service) fn notify(&self) {
        self.receipt.notify();
    }

    pub(super) fn into_receipt(self) -> Result<SubscriptionObservationReceipt, ()> {
        let shared = self.shared.upgrade().ok_or(())?;
        let policy = shared.registered_policy();
        let closed = self.receipt.is_closed();
        if !policy || closed {
            return Err(());
        }
        Ok(self.receipt)
    }
}

impl Shared {
    pub(super) fn source_delivery(&self) -> Option<Arc<chigiri::__private::HostSourceDelivery>> {
        self.source.get().cloned()
    }
    fn registered_policy(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.decision.is_none() && matches!(state.phase, Phase::Registered { .. })
        })
    }
}
