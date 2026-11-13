use std::collections::HashSet;
use std::sync::Arc;

use chigiri_runtime::{ErrorIdFactory, RequestIdFactory};

use super::{
    DeterministicErrorIdFactory, RandomErrorIdFactory, RandomIdError, RandomRequestIdFactory,
    encode_hex,
};

#[test]
fn deterministic_factory_returns_noncolliding_sequence_values() {
    let factory = DeterministicErrorIdFactory::new(1);
    assert_ne!(factory.create().unwrap(), factory.create().unwrap());
}

#[test]
fn random_factory_converts_128_bits_directly_to_lowercase_hex() {
    let encoded = encode_hex([
        0x00, 0x01, 0x0f, 0x10, 0x2a, 0x7f, 0x80, 0xff, 0x55, 0xaa, 0x12, 0x34, 0x56, 0x78, 0x9a,
        0xbc,
    ])
    .unwrap();
    assert_eq!(encoded, "00010f102a7f80ff55aa123456789abc");

    let request_id = RandomRequestIdFactory::new().unwrap().create().unwrap();
    let error_id = RandomErrorIdFactory.create().unwrap();
    for value in [request_id.as_str(), error_id.as_str()] {
        assert_eq!(value.len(), 32);
        assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(value, value.to_ascii_lowercase());
    }
}

#[test]
fn request_factory_returns_deterministic_values_from_a_fixed_seed_and_counter() {
    let factory = RandomRequestIdFactory::from_seed_and_counter([0x42; 32], 7);
    assert_eq!(
        factory.create().unwrap().as_str(),
        "1247483410f9aa3f54fcf7e16c29ce55"
    );
}

#[test]
fn request_factory_distinguishes_different_seeds() {
    let first = RandomRequestIdFactory::from_seed_and_counter([0x11; 32], 0);
    let second = RandomRequestIdFactory::from_seed_and_counter([0x22; 32], 0);
    assert_ne!(first.create().unwrap(), second.create().unwrap());
}

#[test]
fn request_factory_clones_share_the_counter() {
    let seed = [0x33; 32];
    let factory = RandomRequestIdFactory::from_seed_and_counter(seed, 0);
    let cloned = factory.clone();
    let expected = RandomRequestIdFactory::from_seed_and_counter(seed, 0);
    assert_eq!(factory.create().unwrap(), expected.create().unwrap());
    assert_eq!(cloned.create().unwrap(), expected.create().unwrap());
}

#[test]
fn request_factory_produces_unique_ids_across_eight_parallel_producers() {
    const PRODUCERS: usize = 8;
    const PER_PRODUCER: usize = 1_024;

    let factory = Arc::new(RandomRequestIdFactory::from_seed_and_counter([0x44; 32], 0));
    let handles = (0..PRODUCERS)
        .map(|_| {
            let factory = Arc::clone(&factory);
            std::thread::spawn(move || {
                (0..PER_PRODUCER)
                    .map(|_| factory.create().unwrap().as_str().to_owned())
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let values = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), PRODUCERS * PER_PRODUCER);
    assert_eq!(values.iter().collect::<HashSet<_>>().len(), values.len());
}

#[test]
fn request_factory_fails_closed_on_counter_wraparound() {
    let factory = RandomRequestIdFactory::from_seed_and_counter([0x55; 32], u64::MAX);
    assert!(factory.create().is_err());
    assert!(factory.create().is_err());
}

#[test]
fn request_factory_construction_fails_when_seed_acquisition_fails() {
    let result = RandomRequestIdFactory::from_seed_source(|_| Err(RandomIdError::Random));
    assert!(result.is_err());
}

#[test]
fn request_factory_rejects_process_and_counter_overflow() {
    let factory = RandomRequestIdFactory::from_seed_and_counter([0x57; 32], 9);
    let other_process = std::process::id().wrapping_add(1);
    assert!(factory.create_for_process(other_process).is_err());
    assert!(factory.create_for_process(other_process).is_err());
    let expected = RandomRequestIdFactory::from_seed_and_counter([0x57; 32], 9);
    assert_eq!(factory.create().unwrap(), expected.create().unwrap());
}

#[test]
fn request_factory_debug_redacts_seed_counter_and_ids() {
    let seed = [0x61; 32];
    let factory = RandomRequestIdFactory::from_seed_and_counter(seed, 123_456_789);
    let generated = factory.create().unwrap();
    let debug = format!("{factory:?}");
    assert_eq!(debug, "RandomRequestIdFactory { .. }");
    assert!(!debug.contains("6161616161616161"));
    assert!(!debug.contains("123456789"));
    assert!(!debug.contains(generated.as_str()));
}
