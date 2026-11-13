//! Forced parent termination is never a successful budget outcome.

use super::support::{Server, edited, request};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[test]
fn mock_12_vm_loop_memory_and_host_budget_recover_same_server() {
    let _test_case = crate::support::test_case();
    for (case, failure) in [
        ("loop", "while true do end"),
        (
            "caught_loop",
            "while true do pcall(function() while true do end end) end",
        ),
        ("memory", "local s = \"x\" for i = 1, 30 do s = s .. s end"),
        (
            "caught_memory",
            "pcall(function() local s = \"x\" for i = 1, 30 do s = s .. s end end)",
        ),
        (
            "host",
            "for i = 1, 256 do ctx.models.User:find({limit=0}) end",
        ),
    ] {
        let body = format!(
            "local user = ctx.models.User:create(args.input)\nif args.input.name == \"fail\" then {failure} end\nreturn user"
        );
        let mut server = Server::start(&edited("Mutation.createUser", &body));
        let started = Instant::now();
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &json!({"query":"mutation{createUser(input:{name:\"fail\"}){id}}"}).to_string(),
        );
        assert_eq!(response.status, 500, "{case}: {}", response.body);
        let failed: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(failed["errors"].as_array().unwrap().len(), 1);
        assert!(failed["data"].is_null());
        let (code, message) = if case.contains("loop") {
            ("MOCK_EXECUTION_TIMEOUT", "Mock execution timed out")
        } else {
            (
                "MOCK_EXECUTION_LIMIT_EXCEEDED",
                "Mock execution limit exceeded",
            )
        };
        assert_eq!(failed["errors"][0]["extensions"]["code"], code);
        assert_eq!(failed["errors"][0]["message"], message);
        if !case.contains("loop") {
            assert_eq!(
                failed["errors"][0]["extensions"]["limit"],
                if case == "host" {
                    "hostCalls"
                } else {
                    "vmMemoryBytes"
                }
            );
        }
        assert!(started.elapsed() < Duration::from_secs(2), "{case}");
        if case.contains("loop") {
            assert!(started.elapsed() >= Duration::from_millis(450));
        }
        println!("{case}: {:?}; {failed}", started.elapsed());
        assert_eq!(
            server.graphql("{getUsers{id}}")["data"]["getUsers"],
            json!([{"id":"User-1"},{"id":"User-2"}])
        );
        assert_eq!(
            server.graphql("mutation{createUser(input:{name:\"ok\"}){id}}")["data"]["createUser"]["id"],
            "User-3"
        );
        server.stop("-TERM");
    }
}

#[test]
fn mock_12_host_limit_inclusive_256_and_globals_reset_each_root() {
    let _test_case = crate::support::test_case();
    let body = "local user = ctx.models.User:create({id='User-3',name=args.input.name}) for i = 1, 255 do ctx.models.User:find({limit=0}) end return user";
    let mut server = Server::start(&edited("Mutation.createUser", body));
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"boundary\"}){id}}")["data"]["createUser"]
            ["id"],
        "User-3"
    );
    server.stop("-TERM");
    let body = "local globals = _G :: any if globals.saved ~= nil then error('global retained') end globals.saved = 1 return ctx.models.User:find({limit=args.limit})";
    let mut server = Server::start(&edited("Query.getUsers", body));
    for _ in 0..2 {
        assert_eq!(
            server.graphql("{getUsers{id}}")["data"]["getUsers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    server.stop("-TERM");
}

#[test]
fn mock_13_busy_disconnect_noncommit_and_signal_during_vm() {
    let _test_case = crate::support::test_case();
    let body = "local user = ctx.models.User:create(args.input) if args.input.name == \"wait\" then while true do end end return user";
    let mut server = Server::start(&slow_config(&edited("Mutation.createUser", body)));
    let mut connection = begin_slow(&server);
    assert_waiting(&connection);
    connection.flush().unwrap();
    drop(connection);
    let cancelled = Instant::now();
    loop {
        let response = request(
            server.address,
            "POST",
            &[("Content-Type", "application/json")],
            "{\"query\":\"{getUsers{id}}\"}",
        );
        if response.status == 200 {
            break;
        }
        assert_eq!(response.status, 503);
        assert!(
            cancelled.elapsed() < Duration::from_millis(400),
            "disconnect did not cancel the VM before its deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(cancelled.elapsed() < Duration::from_millis(400));
    println!(
        "disconnect cancellation and recovery: {:?}",
        cancelled.elapsed()
    );
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"],
        json!([{"id":"User-1"},{"id":"User-2"}])
    );
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"ok\"}){id}}")["data"]["createUser"]["id"],
        "User-3"
    );
    let connection = begin_slow(&server);
    assert_waiting(&connection);
    server.stop("-TERM");
    drop(connection);
}

pub(super) fn slow_config(config: &str) -> String {
    super::hooks_http::with_hook(
        config,
        chigiri_mock::hooks::HookSlot::FieldAfter,
        "if ctx.field.path[1] == 'waiting' then ctx.log('info','vm-waiting') while true do end end return nil",
    )
}

pub(super) fn begin_slow(server: &Server) -> TcpStream {
    let previous = waiting_count(server);
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    let body =
        json!({"query":"mutation{waiting:createUser(input:{name:\"held\"}){id}}"}).to_string();
    write!(stream, "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
    // Observe the request at 100 Continue, then use hook logs to detect VM startup.
    let mut ready = Vec::new();
    while !ready.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        ready.push(byte[0]);
        assert!(ready.len() < 1024);
    }
    assert!(String::from_utf8(ready).unwrap().contains("100 Continue"));
    stream.write_all(body.as_bytes()).unwrap();
    stream.flush().unwrap();
    let deadline = Instant::now() + Duration::from_millis(400);
    while waiting_count(server) == previous {
        assert!(
            Instant::now() < deadline,
            "VM did not start after candidate registration."
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_waiting(&stream);
    stream
}

fn waiting_count(server: &Server) -> usize {
    std::fs::read_to_string(server.directory.0.join("stdout.log"))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["message"] == "vm-waiting")
        .count()
}

pub(super) fn assert_waiting(stream: &TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(2)))
        .unwrap();
    let mut byte = [0];
    let error = stream
        .peek(&mut byte)
        .expect_err("response bytes arrived while the VM should still be waiting");
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
}
