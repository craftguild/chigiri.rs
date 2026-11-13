//! Charge shared physical source ownership before allocating or copying it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One server's source budget, independent of encoded response accounting.
#[derive(Clone, Debug)]
pub(crate) struct Retention {
    state: Arc<State>,
}

#[derive(Debug)]
struct State {
    limit: usize,
    used: AtomicUsize,
}

/// Move-only ownership; sharing the containing capture keeps its charge alive.
#[derive(Debug)]
pub(crate) struct Reservation {
    owner: Retention,
    bytes: usize,
    limit: usize,
}

impl Retention {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            state: Arc::new(State {
                limit,
                used: AtomicUsize::new(0),
            }),
        }
    }

    pub(crate) fn reserve(&self, bytes: usize, limit: usize) -> Result<Reservation, ()> {
        let mut reservation = Reservation {
            owner: self.clone(),
            bytes: 0,
            limit,
        };
        reservation.grow(bytes)?;
        Ok(reservation)
    }

    /// Admission preflight does not promise capacity to future producer ticks.
    pub(crate) fn fits(&self, additional: usize) -> bool {
        self.used()
            .checked_add(additional)
            .is_some_and(|bytes| bytes <= self.state.limit)
    }

    pub(crate) fn used(&self) -> usize {
        self.state.used.load(Ordering::Acquire)
    }

    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Reservation {
    pub(crate) fn grow(&mut self, additional: usize) -> Result<(), ()> {
        let next = self.bytes.checked_add(additional).ok_or(())?;
        if next > self.limit {
            return Err(());
        }
        self.owner
            .state
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(additional)
                    .filter(|used| *used <= self.owner.state.limit)
            })
            .map_err(|_| ())?;
        self.bytes = next;
        Ok(())
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn belongs_to(&self, owner: &Retention) -> bool {
        self.owner.same_owner(owner)
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.owner
            .state
            .used
            .fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ev_09_shared_and_local_limits_are_inclusive_and_release_on_last_owner() {
        let budget = Retention::new(10);
        let mut first = budget.reserve(4, 6).unwrap();
        let second = Arc::new(budget.reserve(4, 4).unwrap());
        assert!(first.belongs_to(&budget));
        assert!(!first.belongs_to(&Retention::new(10)));
        first.grow(2).unwrap();
        assert_eq!(budget.used(), 10);
        assert!(first.grow(1).is_err());
        assert!(budget.reserve(1, 1).is_err());
        assert_eq!(budget.used(), 10);
        let last = second.clone();
        drop(second);
        assert_eq!(budget.used(), 10);
        drop(last);
        assert_eq!(budget.used(), 6);
        assert!(budget.fits(4));
        assert!(!budget.fits(5));
        first.grow(usize::MAX).unwrap_err();
        assert_eq!(first.bytes(), 6);
        drop(first);
        assert_eq!(budget.used(), 0);
        let exact = budget.reserve(10, 10).unwrap();
        assert_eq!(exact.bytes(), 10);
    }
}
