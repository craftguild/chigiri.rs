//! Hold unfinished TCP requests while verifying concurrent admission, rejection, disconnect, and recovery.

use crate::support::{Server, TEMPLATE, request};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn pending(server: &Server) -> TcpStream {
    header(server, 100)
}

fn header(server: &Server, length: usize) -> TcpStream {
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(stream, "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nConnection: close\r\nContent-Length: {length}\r\n\r\n").unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
        assert!(header.len() < 1024);
    }
    assert!(String::from_utf8(header).unwrap().contains("100 Continue"));
    stream
}

#[test]
fn soft_elapsed_warning_still_allows_mutation_commit() {
    let _case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    let body =
        serde_json::json!({"query":"mutation{createUser(input:{name:\"elapsed-soft\"}){id}}"})
            .to_string();
    let mut stream = header(&server, body.len());
    let deadline = Instant::now() + Duration::from_millis(4500);
    let warning = loop {
        let stderr = std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
        if let Some(warning) = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|record| record["kind"] == "limit_warning" && record["limit"] == "elapsedMs")
        {
            break warning;
        }
        assert!(
            Instant::now() < deadline,
            "Soft alert is not received: {stderr}"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    stream.write_all(body.as_bytes()).unwrap();
    let response = crate::support::read_response(stream, "POST").unwrap();
    assert_eq!(response.status, 200, "{}", response.body);
    let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(response.get("errors").is_none());
    assert_eq!(response["data"]["createUser"]["id"], "User-3");
    assert!(
        server.graphql("{getUsers{id name}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "User-3" && row["name"] == "elapsed-soft")
    );
    server.stop("-TERM");
    let stderr = std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let same: Vec<serde_json::Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["request_id"] == warning["request_id"])
        .collect();
    assert_eq!(
        same.iter()
            .filter(|record| record["kind"] == "limit_warning" && record["limit"] == "elapsedMs")
            .count(),
        1
    );
    let terminal = same
        .iter()
        .find(|record| record["kind"] == "request_terminal")
        .unwrap();
    assert_eq!(terminal["status"], 200);
    assert_eq!(terminal["transaction"], "committed");
    assert_eq!(terminal["cause"], "success");
}

#[test]
fn unfinished_http_bodies_allow_concurrency_enforce_capacity_and_recover_after_disconnect() {
    let _case = crate::support::test_case();
    let mut server = Server::with_graphiql(TEMPLATE, false);
    let mut held = vec![pending(&server)];
    let query = || {
        request(
            server.address,
            "POST",
            &[("Content-Type", "application/json")],
            "{\"query\":\"{getUsers{id}}\"}",
        )
    };
    let concurrent = query();
    assert_eq!(concurrent.status, 200);
    assert!(
        serde_json::from_str::<serde_json::Value>(&concurrent.body).unwrap()["data"]["getUsers"]
            .is_array()
    );
    for _ in 1..chigiri_mock::limits::HTTP_REQUESTS {
        held.push(pending(&server));
    }
    assert_eq!(query().status, 503);
    for path in ["/graphiql", "/graphiql/assets/app.js"] {
        assert_eq!(
            crate::support::request_path(server.address, "GET", path, &[], "").status,
            200
        );
    }
    assert_eq!(query().status, 503);
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let status = query().status;
        if status == 200 {
            break;
        }
        assert_eq!(status, 503);
        assert!(
            Instant::now() < deadline,
            "the released admission slot must become available"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    server.stop("-TERM");
}

#[test]
fn disconnecting_first_request_allows_second_mutation_to_complete() {
    let _case = crate::support::test_case();
    let mut server = Server::start(&crate::budgets::slow_config(TEMPLATE));
    let first = crate::budgets::begin_slow(&server);
    let body =
        serde_json::json!({"query":"mutation{createUser(input:{name:\"second\"}){id name}}"})
            .to_string();
    let mut second = header(&server, body.len());
    second.write_all(body.as_bytes()).unwrap();
    second.flush().unwrap();
    crate::budgets::assert_waiting(&second);
    crate::budgets::assert_waiting(&first);
    drop(first);
    let started = Instant::now();
    let mut response = String::new();
    second
        .take(64 * 1024)
        .read_to_string(&mut response)
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(400));
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response.contains("second") && response.contains("User-3"),
        "{response}"
    );
    let rows = server.graphql("{getUsers{id name}}");
    assert_eq!(
        rows["data"]["getUsers"],
        serde_json::json!([
            {"id":"User-1","name":"Alice"}, {"id":"User-2","name":"Bob"},
            {"id":"User-3","name":"second"}
        ])
    );
    server.stop("-TERM");
}
