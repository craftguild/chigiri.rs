//! Exact counters used by JavaScript-visible delivery metadata and generations.

/// Largest integer represented exactly by the supported JSON clients.
pub(crate) const MAX_EXACT_JSON_INTEGER: u64 =
    chigiri_runtime::__private::HostSourceDelivery::MAX_DROPPED_TOTAL;

/// Failure to advance a bounded counter without losing precision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CounterError;

/// A counter that never wraps or exceeds the exact JSON integer ceiling.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CheckedCounter {
    value: u64,
}

impl CheckedCounter {
    pub(crate) const fn new() -> Self {
        Self { value: 0 }
    }

    pub(crate) const fn value(self) -> u64 {
        self.value
    }

    #[cfg(test)]
    pub(crate) fn increment(&mut self) -> Result<u64, CounterError> {
        self.add(1)
    }

    pub(crate) fn increment_with<R, E>(
        &mut self,
        transition: impl FnOnce() -> Result<R, E>,
    ) -> Result<Result<R, E>, CounterError> {
        let next = checked_value(self.value, 1)?;
        let result = transition();
        if result.is_ok() {
            self.value = next;
        }
        Ok(result)
    }

    pub(crate) fn add(&mut self, amount: u64) -> Result<u64, CounterError> {
        let next = checked_value(self.value, amount)?;
        self.value = next;
        Ok(next)
    }

    #[cfg(test)]
    pub(super) const fn from_value(value: u64) -> Self {
        Self { value }
    }
}

fn checked_value(value: u64, amount: u64) -> Result<u64, CounterError> {
    let next = value.checked_add(amount).ok_or(CounterError)?;
    if next > MAX_EXACT_JSON_INTEGER {
        return Err(CounterError);
    }
    Ok(next)
}
