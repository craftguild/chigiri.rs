//! Test decimal multiples, integer precision, and conservative significant-digit counting.

use super::{decimal, work::Work};

#[test]
fn sd_13_multiple_of_uses_canonical_decimal_without_epsilon() {
    for (number, divisor, expected) in [
        (0.3, 0.1, true),
        (-0.3, 0.1, true),
        (0.0, 0.1, true),
        (-0.0, 0.1, true),
        (0.31, 0.1, false),
        (-0.31, 0.1, false),
        (0.1 + 0.2, 0.1, false),
        (3.0, 0.5, true),
        (3.0, 2.0, false),
        (1_152_921_504_606_846_976.0, 2.0, true),
        (1_152_921_504_606_846_976.0, 3.0, false),
        (f64::from_bits(1), f64::from_bits(1), true),
        (f64::MAX, 1.0, true),
        (f64::MAX, 3.0, false),
    ] {
        assert_eq!(
            decimal::multiple(number, divisor, &mut Work::default()).unwrap(),
            expected,
            "{number} / {divisor}"
        );
    }
    for divisor in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(decimal::multiple(1.0, divisor, &mut Work::default()).is_err());
    }
}

#[test]
fn sd_12_multiple_of_interval_includes_integer_domain() {
    for (low, high, step, integer, expected) in [
        (0.1, 0.2, 0.3, false, false),
        (0.1, 0.3, 0.3, false, true),
        (-0.2, -0.1, 0.3, false, false),
        (-0.3, -0.1, 0.3, false, true),
        (1.0, 1.0, 0.5, true, true),
        (1.0, 2.0, 0.3, true, false),
        (1.0, 3.0, 0.3, true, true),
        (-2.0, -1.0, 0.3, true, false),
        (-3.0, -1.0, 0.3, true, true),
        (1.0, 1.0, 0.125, true, true),
    ] {
        assert_eq!(
            decimal::intersects(low, high, step, integer, &mut Work::default()).unwrap(),
            expected
        );
    }
}

#[test]
fn sd_14_decimal_work_is_reserved_and_sticky_across_clones() {
    let mut work = Work::default();
    let mut shared = work.clone();
    work.charge(super::work::MAX_WORK - 1).unwrap();
    assert!(decimal::multiple(0.3, 0.1, &mut shared).is_err());
    assert!(work.charge(0).is_err());
}
