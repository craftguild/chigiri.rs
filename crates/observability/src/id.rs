//! Error and request ID factories that keep runtime dependencies out of core.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chigiri_runtime::{
    ErrorId, ErrorIdFactory, ErrorIdFactoryError, RequestId, RequestIdFactory,
    RequestIdFactoryError,
};

/// Deterministic sequential error-ID factory for reproducible tests.
#[derive(Debug, Default)]
pub struct DeterministicErrorIdFactory {
    next: AtomicU64,
}

impl DeterministicErrorIdFactory {
    pub const fn new(first: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
        }
    }
}

impl ErrorIdFactory for DeterministicErrorIdFactory {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        let value = self.next.fetch_add(1, Ordering::Relaxed);
        ErrorId::new(format!("error-{value:016x}"))
            .map_err(|_| ErrorIdFactoryError::new(FactoryInvariant))
    }
}

/// Deterministic sequential request-ID factory for reproducible tests.
#[derive(Debug, Default)]
pub struct DeterministicRequestIdFactory {
    next: AtomicU64,
}

impl DeterministicRequestIdFactory {
    pub const fn new(first: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
        }
    }
}

impl RequestIdFactory for DeterministicRequestIdFactory {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let value = self.next.fetch_add(1, Ordering::Relaxed);
        RequestId::new(format!("request-{value:016x}"))
            .map_err(|_| RequestIdFactoryError::new(RequestFactoryInvariant))
    }
}

/// Production factory that creates 128-bit opaque IDs from an OS random source.
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomErrorIdFactory;

impl ErrorIdFactory for RandomErrorIdFactory {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        random_hex()
            .and_then(|value| ErrorId::new(value).map_err(|_| RandomIdError::Invariant))
            .map_err(|error| match error {
                RandomIdError::Random => ErrorIdFactoryError::new(RandomFailure),
                RandomIdError::Invariant => ErrorIdFactoryError::new(FactoryInvariant),
            })
    }
}

/// Production factory that derives request IDs from a startup seed and a counter shared by clones.
#[derive(Clone)]
pub struct RandomRequestIdFactory {
    state: Arc<RandomRequestIdState>,
}

impl RandomRequestIdFactory {
    /// Obtains one 256-bit seed from the OS after a worker fork and before accepting requests.
    pub fn new() -> Result<Self, RequestIdFactoryError> {
        Self::from_seed_source(|seed| getrandom::fill(seed).map_err(|_| RandomIdError::Random))
    }

    fn from_seed_source(
        source: impl FnOnce(&mut [u8; 32]) -> Result<(), RandomIdError>,
    ) -> Result<Self, RequestIdFactoryError> {
        let mut seed = [0u8; 32];
        source(&mut seed).map_err(|_| RequestIdFactoryError::new(RandomFailure))?;
        Ok(Self::from_seed_and_counter(seed, 0))
    }

    fn from_seed_and_counter(seed: [u8; 32], counter: u64) -> Self {
        Self {
            state: Arc::new(RandomRequestIdState {
                seed,
                next: AtomicU64::new(counter),
                creator_process_id: std::process::id(),
            }),
        }
    }

    fn create_for_process(&self, process_id: u32) -> Result<RequestId, RequestIdFactoryError> {
        if process_id != self.state.creator_process_id {
            return Err(RequestIdFactoryError::new(ProcessChanged));
        }
        let counter = self.next_counter()?;
        request_id_hex(&self.state.seed, counter)
            .and_then(|value| RequestId::new(value).map_err(|_| RandomIdError::Invariant))
            .map_err(|error| match error {
                RandomIdError::Random => RequestIdFactoryError::new(RandomFailure),
                RandomIdError::Invariant => RequestIdFactoryError::new(RequestFactoryInvariant),
            })
    }

    fn next_counter(&self) -> Result<u64, RequestIdFactoryError> {
        self.state
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| RequestIdFactoryError::new(CounterExhausted))
    }
}

impl fmt::Debug for RandomRequestIdFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RandomRequestIdFactory")
            .finish_non_exhaustive()
    }
}

impl RequestIdFactory for RandomRequestIdFactory {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        self.create_for_process(std::process::id())
    }
}

struct RandomRequestIdState {
    seed: [u8; 32],
    next: AtomicU64,
    creator_process_id: u32,
}

fn request_id_hex(seed: &[u8; 32], counter: u64) -> Result<String, RandomIdError> {
    const DOMAIN: &[u8; 21] = b"chigiri-request-id-v1";

    let mut input = [0u8; DOMAIN.len() + std::mem::size_of::<u64>()];
    input[..DOMAIN.len()].copy_from_slice(DOMAIN);
    input[DOMAIN.len()..].copy_from_slice(&counter.to_be_bytes());
    let digest = blake3::keyed_hash(seed, &input);
    let mut value = [0u8; 16];
    value.copy_from_slice(&digest.as_bytes()[..16]);
    encode_hex(value)
}

fn random_hex() -> Result<String, RandomIdError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| RandomIdError::Random)?;
    encode_hex(bytes)
}

fn encode_hex(bytes: [u8; 16]) -> Result<String, RandomIdError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = vec![0u8; 32];
    for (index, byte) in bytes.into_iter().enumerate() {
        encoded[index * 2] = HEX[usize::from(byte >> 4)];
        encoded[index * 2 + 1] = HEX[usize::from(byte & 0x0f)];
    }
    String::from_utf8(encoded).map_err(|_| RandomIdError::Invariant)
}

#[derive(Debug)]
enum RandomIdError {
    Random,
    Invariant,
}

#[derive(Debug)]
struct FactoryInvariant;

impl fmt::Display for FactoryInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("generated error ID violated its own format")
    }
}

impl std::error::Error for FactoryInvariant {}

#[derive(Debug)]
struct RequestFactoryInvariant;

impl fmt::Display for RequestFactoryInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("generated request ID violated its own format")
    }
}

impl std::error::Error for RequestFactoryInvariant {}

struct RandomFailure;

impl fmt::Debug for RandomFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RandomFailure")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RandomFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OS random source failed")
    }
}

impl std::error::Error for RandomFailure {}

struct CounterExhausted;

impl fmt::Debug for CounterExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CounterExhausted")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for CounterExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request ID counter exhausted")
    }
}

impl std::error::Error for CounterExhausted {}

struct ProcessChanged;

impl fmt::Debug for ProcessChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessChanged")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ProcessChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request ID factory cannot cross a process boundary")
    }
}

impl std::error::Error for ProcessChanged {}

#[cfg(test)]
#[path = "id/tests.rs"]
mod tests;
