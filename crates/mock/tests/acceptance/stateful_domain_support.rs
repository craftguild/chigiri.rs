//! HTTP assertions shared by the stateful authoring acceptance scenario.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::support::Server;

pub(super) struct Reply {
    pub status: u16,
    pub value: Value,
}

pub(super) fn operation(
    server: &Server,
    token: Option<&str>,
    query: &str,
    extra: &[(&str, &str)],
) -> Reply {
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(token) = token {
        headers.push(("x-session", token));
    }
    headers.extend_from_slice(extra);
    let response = server.http("POST", &headers, &json!({"query":query}).to_string());
    Reply {
        status: response.status,
        value: serde_json::from_str(&response.body).unwrap(),
    }
}

pub(super) fn data<'a>(reply: &'a Reply, field: &str) -> &'a Value {
    assert_eq!(reply.status, 200, "{}", reply.value);
    assert!(reply.value.get("errors").is_none(), "{}", reply.value);
    &reply.value["data"][field]
}

pub(super) fn business(reply: &Reply, code: &str) {
    assert_eq!(reply.status, 200, "{}", reply.value);
    assert!(reply.value["data"].is_null());
    assert_eq!(reply.value["errors"][0]["extensions"]["code"], code);
    assert!(reply.value["errors"][0]["extensions"]["requestId"].is_string());
}

pub(super) fn wait_for_diagnostic(server: &Server, coordinate: &str, request_id: &str) -> Value {
    let path = server.directory.0.join("stderr.log");
    let request_id = request_id.parse::<u64>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stderr = fs::read_to_string(&path).unwrap();
        if let Some(record) = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|record| {
                record["kind"] == "resolver_diagnostic"
                    && record["coordinate"] == coordinate
                    && record["request_id"].as_u64() == Some(request_id)
            })
        {
            return record;
        }
        assert!(Instant::now() < deadline, "missing {coordinate} diagnostic");
        thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn me(server: &Server, token: &str) -> Value {
    data(
        &operation(server, Some(token), "{me{id name wallet{id balance}}}", &[]),
        "me",
    )
    .clone()
}

pub(super) fn authoring_view(server: &Server, token: &str, viewer: &str) -> Value {
    let reply = operation(
        server,
        Some(token),
        "{me{id images{id url viewerLabel} featuredImage{id url viewerLabel} wallet{id balance rewards{edges{node{id post{id}}}}}} feed{edges{node{id reward{id}}}}}",
        &[("x-viewer", viewer)],
    );
    assert_eq!(reply.status, 200, "{}", reply.value);
    assert!(reply.value.get("errors").is_none(), "{}", reply.value);
    reply.value["data"].clone()
}

pub(super) fn upload(server: &Server, token: &str) -> Value {
    data(
        &operation(
            server,
            Some(token),
            "{upload(id:\"upload-1\"){id myVote completed}}",
            &[],
        ),
        "upload",
    )
    .clone()
}

pub(super) fn transfer(
    server: &Server,
    token: &str,
    key: &str,
    amount: i32,
    extra: &[(&str, &str)],
) -> Value {
    let query =
        format!("mutation{{transfer(toId:\"u2\",amount:{amount},key:\"{key}\"){{id amount}}}}");
    data(&operation(server, Some(token), &query, extra), "transfer").clone()
}

pub(super) fn assert_retry_contract(
    server: &Server,
    first: &str,
    second: &str,
    key: &str,
    id: &str,
    balances: (i64, i64),
    history: usize,
) {
    let committed = transfer(server, first, key, 5, &[]);
    assert_eq!(committed, json!({"id":id,"amount":5}));
    assert_eq!(transfer(server, first, key, 5, &[]), committed);
    let conflict = format!("mutation{{transfer(toId:\"u2\",amount:6,key:\"{key}\"){{id}}}}");
    business(
        &operation(server, Some(first), &conflict, &[]),
        "IDEMPOTENCY_CONFLICT",
    );
    assert_balances_and_history(server, first, second, balances.0, balances.1, history);
}

pub(super) fn assert_balances_and_history(
    server: &Server,
    first: &str,
    second: &str,
    first_balance: i64,
    second_balance: i64,
    count: usize,
) {
    assert_eq!(me(server, first)["wallet"]["balance"], first_balance);
    assert_eq!(me(server, second)["wallet"]["balance"], second_balance);
    let history = operation(
        server,
        Some(first),
        "{history{id amount idempotencyKey}}",
        &[],
    );
    assert_eq!(data(&history, "history").as_array().unwrap().len(), count);
}
