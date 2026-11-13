//! Actual root progress waits for flush and the last immutable payload owner.

use super::super::state::State;
use super::support::{Fixture, kind, messages_for};

#[test]
fn retained_old_physical_event_blocks_writer_not_same_id_admission() {
    let mut fixture = Fixture::new(State::new(Vec::new()));
    fixture.socket.0.lock().unwrap().retain_next = true;
    fixture.socket.subscribe("reuse", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|driver| {
        messages_for(&handle, "reuse").len() == 1
            && driver
                .operation_state_for_test("reuse")
                .is_some_and(|(_, live, _)| !live)
    });
    let old = fixture
        .driver
        .as_ref()
        .unwrap()
        .operation_state_for_test("reuse")
        .unwrap()
        .0;
    let bytes = fixture
        .socket
        .0
        .lock()
        .unwrap()
        .retained
        .as_ref()
        .unwrap()
        .as_bytes()
        .len();
    assert_eq!(
        fixture.driver.as_ref().unwrap().encoded_bytes_for_test(),
        bytes
    );
    fixture.socket.subscribe("reuse", "query { ping }");
    fixture.until(|driver| {
        driver
            .operation_state_for_test("reuse")
            .is_some_and(|(generation, _, queued)| generation != old && queued)
    });
    assert_eq!(messages_for(&fixture.socket, "reuse").len(), 1);
    let retained = fixture.socket.0.lock().unwrap().retained.take();
    let charged = fixture.driver.as_ref().unwrap().encoded_bytes_for_test();
    drop(retained);
    // Replacement already cancelled the old common controller. Its original
    // physical frame is now the last old byte owner; release only those bytes,
    // never the replacement's independently queued result or its charge.
    assert_eq!(
        fixture.driver.as_ref().unwrap().encoded_bytes_for_test(),
        charged - bytes
    );
    let handle = fixture.socket.clone();
    fixture.until(|driver| {
        messages_for(&handle, "reuse").len() == 3 && driver.encoded_bytes_for_test() == 0
    });
    let messages = messages_for(&fixture.socket, "reuse");
    assert_eq!(
        messages
            .iter()
            .map(|message| kind(message))
            .collect::<Vec<_>>(),
        ["next", "next", "complete"]
    );
    fixture.finish();
}
