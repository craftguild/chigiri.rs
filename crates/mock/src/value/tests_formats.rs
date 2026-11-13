//! Verifies exact grammar boundaries for the eight built-in formats.

use super::{ValueBudget, formats};
use crate::ir::Format;

#[test]
fn sd_12_formats_accept_only_the_fixed_grammar() {
    for (format, yes, no) in [
        (
            Format::Byte,
            vec!["", "Zg==", "Zm9v", "+/8="],
            vec!["Zg", "Zh==", "Zm9=", "-w==", "Zg==\n", " Zg=="],
        ),
        (
            Format::Date,
            vec!["0001-01-01", "2000-02-29", "2024-02-29", "9999-12-31"],
            vec![
                "0000-01-01",
                "1900-02-29",
                "2023-02-29",
                "2024-13-01",
                "2024-01-00",
                "2024",
                "2024-01-01Z",
            ],
        ),
        (
            Format::DateTime,
            vec![
                "2024-02-29t23:59:59.123456789012z",
                "2024-01-01T00:00:00+09:00",
                "2024-01-01T00:00:00-00:00",
                "2024-01-01T00:00:00+23:59",
            ],
            vec![
                "2024-01-01 00:00:00Z",
                "2024-01-01T00:00:00",
                "2024-01-01T24:00:00Z",
                "2024-01-01T00:00:60Z",
                "2024-01-01T00:00:00.Z",
                "2023-02-29T00:00:00Z",
                "2024-01-01T00:00:00+24:00",
            ],
        ),
        (
            Format::Email,
            vec![
                "a+tag@example.com",
                "é@example.com",
                "\"a b\"@example.com",
                "a@xn--r8jz45g.xn--zckzah",
                "a@example.123",
            ],
            vec![
                "A <a@example.com>",
                "a(comment)@example.com",
                "a@localhost",
                "a@under_score.com",
                "a@éxample.jp",
                "a@[127.0.0.1]",
                "a@example.com.",
                "a\r\n@example.com",
            ],
        ),
        (
            Format::Ipv4,
            vec!["0.0.0.0", "192.0.2.1", "255.255.255.255"],
            vec![
                "01.2.3.4",
                "127.1",
                "256.0.0.1",
                "1.2.3.4:80",
                "1.2.3.4/32",
                " 1.2.3.4",
            ],
        ),
        (
            Format::Ipv6,
            vec!["::", "2001:db8::1", "::ffff:192.0.2.1", "ABCD::1"],
            vec!["[::1]", "fe80::1%eth0", "::1/128", "1::2::3", " ::1"],
        ),
        (
            Format::Uri,
            vec![
                "https://example.com/a%20b?q=1#f",
                "mailto:a@example.com",
                "urn:isbn:0451450523",
                "file:///tmp/a",
                "mailto:",
                "https://example.com:65535",
            ],
            vec![
                "example.com",
                "/relative",
                "https://example.com/%ZZ",
                "http://example.com/a b",
                "http://éxample.jp",
                "https://example.com:65536",
            ],
        ),
        (
            Format::Uuid,
            vec![
                "00000000-0000-0000-0000-000000000000",
                "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF",
                "12345678-abcd-0000-FFFF-0123456789ab",
            ],
            vec![
                "00000000000000000000000000000000",
                "urn:uuid:00000000-0000-0000-0000-000000000000",
                "{00000000-0000-0000-0000-000000000000}",
                "G0000000-0000-0000-0000-000000000000",
            ],
        ),
    ] {
        for text in yes {
            assert!(
                formats::validate(format, text, &mut ValueBudget::default()).is_ok(),
                "{}: {text}",
                format.as_str()
            );
        }
        for text in no {
            assert!(
                formats::validate(format, text, &mut ValueBudget::default()).is_err(),
                "{}: {text}",
                format.as_str()
            );
        }
    }
}

#[test]
fn sd_12_format_lengths_are_bounded_before_parsing() {
    let long_fraction = format!("2024-01-01T00:00:00.{}Z", "1".repeat(4075));
    assert_eq!(long_fraction.len(), 4096);
    assert!(
        formats::validate(
            Format::DateTime,
            &long_fraction,
            &mut ValueBudget::default()
        )
        .is_ok()
    );
    assert!(
        formats::validate(
            Format::DateTime,
            &format!("{long_fraction}Z"),
            &mut ValueBudget::default()
        )
        .is_err()
    );
    for (text, expected) in [
        (format!("{}@example.com", "a".repeat(64)), true),
        (format!("{}@example.com", "a".repeat(65)), false),
        (format!("a@{}.com", "a".repeat(63)), true),
        (format!("a@{}.com", "a".repeat(64)), false),
    ] {
        assert_eq!(
            formats::validate(Format::Email, &text, &mut ValueBudget::default()).is_ok(),
            expected
        );
    }
}
