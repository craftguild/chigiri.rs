use std::io::IoSlice;

use chigiri_http::{WebSocketWriteProgressError as Error, WebSocketWriteProgressTracker};

const EMPTY: &[u8] = &[];
const ONE_BYTE: &[u8] = &[0];
const KIB: &[u8] = &[0; 1024];

fn application_bytes(tracker: &mut WebSocketWriteProgressTracker, bytes: &[u8]) -> usize {
    tracker
        .observe_accepted(bytes)
        .expect("the accepted prefix is valid")
        .application_bytes()
}

#[test]
fn short_text_counts_only_payload_and_requires_flush() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    assert_eq!(application_bytes(&mut tracker, &[0x89, 1, b'p']), 0);
    tracker.arm_text(3).unwrap();

    let header = tracker.observe_accepted(&[0x81, 3]).unwrap();
    assert_eq!(header.application_bytes(), 0);
    assert!(!header.payload_complete());
    let first = tracker.observe_accepted(b"a").unwrap();
    assert_eq!(first.application_bytes(), 1);
    assert!(!first.payload_complete());
    let rest = tracker.observe_accepted(b"bc").unwrap();
    assert_eq!(rest.application_bytes(), 2);
    assert!(rest.payload_complete());

    assert_eq!(tracker.arm_text(0), Err(Error::AlreadyArmed));
}

#[test]
fn flush_releases_a_completed_slot_but_never_adds_progress() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(0).unwrap();
    let complete = tracker.observe_accepted(&[0x81, 0]).unwrap();
    assert_eq!(complete.application_bytes(), 0);
    assert!(complete.payload_complete());
    tracker.finish_flush().unwrap();

    tracker.arm_text(1).unwrap();
    assert_eq!(application_bytes(&mut tracker, EMPTY), 0);
    let complete = tracker.observe_accepted(&[0x81, 1, b'x']).unwrap();
    assert_eq!(complete.application_bytes(), 1);
    assert!(complete.payload_complete());
    tracker.finish_flush().unwrap();
}

#[test]
fn two_byte_extended_length_handles_split_header_and_payload() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(126).unwrap();
    assert_eq!(application_bytes(&mut tracker, &[0x81, 126, 0]), 0);
    assert_eq!(application_bytes(&mut tracker, &[126]), 0);
    assert_eq!(application_bytes(&mut tracker, &[0; 125]), 125);
    let final_byte = tracker.observe_accepted(ONE_BYTE).unwrap();
    assert_eq!(final_byte.application_bytes(), 1);
    assert!(final_byte.payload_complete());
    tracker.finish_flush().unwrap();
}

#[test]
fn eight_byte_extended_length_is_checked_without_payload_allocation() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(65_536).unwrap();
    let header = [0x81, 127, 0, 0, 0, 0, 0, 1, 0, 0];
    assert_eq!(application_bytes(&mut tracker, &header), 0);
    for index in 0..64 {
        let progress = tracker.observe_accepted(KIB).unwrap();
        assert_eq!(progress.application_bytes(), 1024);
        assert_eq!(progress.payload_complete(), index == 63);
    }
    tracker.finish_flush().unwrap();
}

#[test]
fn controls_and_text_may_share_one_accepted_prefix() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(2).unwrap();
    let bytes = [0x89, 1, b'p', 0x81, 2, b'o', b'k', 0x8a, 1, b'q', 0x88, 0];
    let progress = tracker.observe_accepted(&bytes).unwrap();
    assert_eq!(progress.application_bytes(), 2);
    assert!(progress.payload_complete());
    tracker.finish_flush().unwrap();
}

#[test]
fn partial_control_may_straddle_arm_and_flush() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    assert_eq!(application_bytes(&mut tracker, &[0x89]), 0);
    tracker.arm_text(2).unwrap();
    let progress = tracker
        .observe_accepted(&[1, b'p', 0x81, 2, b'o', b'k', 0x8a])
        .unwrap();
    assert_eq!(progress.application_bytes(), 2);
    assert!(progress.payload_complete());
    assert_eq!(application_bytes(&mut tracker, &[1, b'q']), 0);
    tracker.finish_flush().unwrap();
}

#[test]
fn a_second_text_before_flush_fails_closed() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(1).unwrap();
    assert_eq!(
        tracker.observe_accepted(&[0x81, 1, b'a', 0x81, 1, b'b']),
        Err(Error::UnexpectedText)
    );
    assert_eq!(tracker.observe_accepted(EMPTY), Err(Error::Failed));
    assert_eq!(tracker.finish_flush(), Err(Error::Failed));
}

#[test]
fn vectored_observation_uses_only_the_reported_prefix() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    tracker.arm_text(4).unwrap();
    let first = [0x89, 0, 0x81];
    let second = [4, b'a', b'b'];
    let third = [b'c', b'd', 0x8a, 0];
    let buffers = [
        IoSlice::new(&first),
        IoSlice::new(&second),
        IoSlice::new(&third),
    ];
    let partial = tracker.observe_accepted_vectored(&buffers, 6).unwrap();
    assert_eq!(partial.application_bytes(), 2);
    assert!(!partial.payload_complete());
    let complete = tracker.observe_accepted(&third).unwrap();
    assert_eq!(complete.application_bytes(), 2);
    assert!(complete.payload_complete());
    tracker.finish_flush().unwrap();
}

#[test]
fn controls_are_safe_before_and_after_any_armed_write() {
    let mut tracker = WebSocketWriteProgressTracker::new();
    let controls = [0x89, 0, 0x8a, 1, b'p', 0x88, 2, 3, 0xe8];
    let progress = tracker.observe_accepted(&controls).unwrap();
    assert_eq!(progress.application_bytes(), 0);
    assert!(!progress.payload_complete());
}

#[test]
fn malformed_or_unexpected_frames_fail_closed() {
    let cases: &[(&[u8], Error)] = &[
        (&[0x81, 0], Error::UnexpectedText),
        (&[0x01, 0], Error::MalformedFrame),
        (&[0x82, 0], Error::MalformedFrame),
        (&[0x81, 0x80], Error::MalformedFrame),
        (&[0x88, 1, 0], Error::MalformedFrame),
        (&[0x89, 126, 0, 126], Error::MalformedFrame),
        (&[0x81, 126, 0, 125], Error::MalformedFrame),
        (
            &[0x81, 127, 0x80, 0, 0, 0, 0, 1, 0, 0],
            Error::LengthOverflow,
        ),
    ];
    for &(bytes, expected) in cases {
        let mut tracker = WebSocketWriteProgressTracker::new();
        if bytes[0] & 0x0f == 1 && expected != Error::UnexpectedText {
            tracker.arm_text(126).unwrap();
        }
        assert_eq!(tracker.observe_accepted(bytes), Err(expected));
        assert_eq!(tracker.observe_accepted(EMPTY), Err(Error::Failed));
    }
}

#[test]
fn exact_length_partial_text_and_vectored_bounds_are_enforced() {
    let mut wrong_length = WebSocketWriteProgressTracker::new();
    wrong_length.arm_text(2).unwrap();
    assert_eq!(
        wrong_length.observe_accepted(&[0x81, 1]),
        Err(Error::TextLengthMismatch)
    );

    let mut partial_text = WebSocketWriteProgressTracker::new();
    partial_text.observe_accepted(&[0x81]).unwrap();
    assert_eq!(partial_text.arm_text(0), Err(Error::UnexpectedText));

    let mut out_of_bounds = WebSocketWriteProgressTracker::new();
    let buffers = [IoSlice::new(&[0x89, 0])];
    assert_eq!(
        out_of_bounds.observe_accepted_vectored(&buffers, 3),
        Err(Error::AcceptedPrefixOutOfBounds)
    );

    if usize::BITS == 64 {
        let mut too_large = WebSocketWriteProgressTracker::new();
        assert_eq!(too_large.arm_text(usize::MAX), Err(Error::LengthOverflow));
    }
}

#[test]
fn flush_rejects_incomplete_text_or_trailing_control() {
    let mut incomplete_text = WebSocketWriteProgressTracker::new();
    incomplete_text.arm_text(2).unwrap();
    incomplete_text.observe_accepted(&[0x81, 2, b'a']).unwrap();
    assert_eq!(incomplete_text.finish_flush(), Err(Error::IncompleteFlush));

    let mut incomplete_control = WebSocketWriteProgressTracker::new();
    incomplete_control.arm_text(1).unwrap();
    incomplete_control
        .observe_accepted(&[0x81, 1, b'a', 0x89, 1])
        .unwrap();
    assert_eq!(
        incomplete_control.finish_flush(),
        Err(Error::IncompleteFlush)
    );
}
