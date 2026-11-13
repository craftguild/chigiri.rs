//! Verify client cancellation and signal interruption produce the expected response and one access record.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

use super::logging::records;
use super::support::{Server, TEMPLATE};

#[test]
fn hk_16_real_disconnect_has_one_cancelled_null_status_and_releases_admission() {
    let _case = crate::support::test_case();
    let config = crate::support::edited(
        "Mutation.createUser",
        "local user = ctx.models.User:create(args.input) if args.input.name == 'wait' then while true do end end return user",
    );
    let mut server = Server::start(&crate::budgets::slow_config(&config));
    let connection = crate::budgets::begin_slow(&server);
    crate::budgets::assert_waiting(&connection);
    drop(connection);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
    loop {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            r#"{"query":"{getUsers{id}}"}"#,
        );
        if response.status == 200 {
            assert_eq!(
                serde_json::from_str::<Value>(&response.body).unwrap()["data"]["getUsers"],
                json!([{"id":"User-1"},{"id":"User-2"}])
            );
            break;
        }
        assert_eq!(response.status, 503);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        std::time::Instant::now() < deadline,
        "Cancel and return before the VM deadline."
    );
    server.stop("-TERM");
    let logs = records(&server);
    let cancelled: Vec<_> = logs
        .iter()
        .filter(|record| record["outcome"] == "cancelled")
        .collect();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0]["method"], "POST");
    assert_eq!(cancelled[0]["path"], "/graphql");
    assert!(cancelled[0]["status"].is_null());
    assert!(cancelled[0]["duration_ms"].as_f64().unwrap() > 0.0);
    let ids: Vec<_> = logs
        .iter()
        .filter(|record| record["kind"] == "access")
        .map(|record| record["request_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
}

#[test]
fn hk_14_16_unfinished_request_body_is_released_on_signal_with_actual_503_access() {
    let _case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    let mut partial = TcpStream::connect(server.address).unwrap();
    partial
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    partial
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    partial.write_all(b"POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100\r\nExpect: 100-continue\r\n\r\n").unwrap();
    let mut received = Vec::new();
    while !received.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        partial.read_exact(&mut byte).unwrap();
        received.push(byte[0]);
        assert!(received.len() < 1024);
    }
    assert!(
        String::from_utf8(received)
            .unwrap()
            .contains("100 Continue")
    );
    server.stop("-TERM");
    let mut response = String::new();
    partial.take(1024).read_to_string(&mut response).unwrap();
    assert!(response.contains("503 Service Unavailable"), "{response}");
    let logs = records(&server);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["status"], 503);
    assert_eq!(logs[0]["outcome"], "completed");
    assert_eq!(logs[0]["path"], "/graphql");
}
