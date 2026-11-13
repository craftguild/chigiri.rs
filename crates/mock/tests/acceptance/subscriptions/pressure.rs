//! Drive a real Mock source error and consume each protocol's terminal wire shape.

use std::thread;
use std::time::Duration;

use serde_json::json;

use super::assertions::Profile;
use super::{http, websocket};
use crate::support::{Response, Server};

const SOURCE_SUBSCRIPTION: &str = r#"subscription { changed(topic: "events") { id padding } }"#;
const MAX_BATCHES: usize = 64;
const PRIMER: &str = "source-0-8";

pub(super) fn exercise(server: &Server) {
    let before = read_count(server);
    let sse = http::hold(
        server.address,
        Profile::ServerSentEvents,
        SOURCE_SUBSCRIPTION,
    );
    let multipart = http::hold(server.address, Profile::Multipart, SOURCE_SUBSCRIPTION);
    let websocket = websocket::hold(server.address, SOURCE_SUBSCRIPTION);
    super::publish(server, "source-active", 12);
    let mut sse = sse.assert_active("source-active");
    let mut multipart = multipart.assert_active("source-active");
    let mut websocket = websocket.assert_active("source-active", None);

    let padding = "x".repeat(4 * 1024);
    let primer = server.operation(json!({
        "query":"mutation($id:ID!,$padding:String!){publish(id:$id,value:8,padding:$padding){id}}",
        "variables":{"id":PRIMER,"padding":padding}
    }));
    assert_eq!(primer["data"]["publish"]["id"], PRIMER);
    // Observe a complete padded event before pressure can select a terminal
    // and abandon pending source work. Do not rely on a burst winning that race.
    let mut observed = [Vec::new(), Vec::new(), Vec::new()];
    for (changed, ids) in [
        sse.next_changed(),
        multipart.next_changed(),
        websocket.next_changed(),
    ]
    .into_iter()
    .zip(&mut observed)
    {
        assert_eq!(changed, json!({"id":PRIMER,"padding":padding}));
        ids.push(changed["id"].as_str().unwrap().to_owned());
    }
    let mut committed = 2_u64;
    let mut rejected = 0;
    for batch in 1..=MAX_BATCHES {
        let prior = before + committed;
        let body = json!({
            "query":"mutation($prefix:String!,$padding:String!){publishBurst(prefix:$prefix,padding:$padding)}",
            "variables":{"prefix":format!("source-{batch}-"),"padding":padding}
        })
        .to_string();
        let response = server.http("POST", &[("Content-Type", "application/json")], &body);
        if response.status == 200 {
            let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(body["data"]["publishBurst"], 8);
            committed += 8;
        } else {
            assert_pressure_failure(&response);
            rejected += 1;
            assert_eq!(
                read_count(server),
                prior,
                "failed candidate must not commit"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    // Source overflow stops the affected recipient; it does not require the
    // already independent publisher to fail. Verify actual wire terminals below.
    assert_eq!((committed - 2) / 8 + rejected, MAX_BATCHES as u64);
    assert_ne!(committed, 2, "pressure must follow committed events");

    let [mut sse_ids, mut multipart_ids, mut websocket_ids] = observed;
    sse_ids.extend(sse.assert_source_failure());
    multipart_ids.extend(multipart.assert_source_failure());
    websocket_ids.extend(websocket.assert_source_failure());
    assert_ordered_events(&sse_ids);
    assert_ordered_events(&multipart_ids);
    assert_ordered_events(&websocket_ids);
    assert_eq!(read_count(server), before + committed);
    for _ in 0..100 {
        if publication_recovered(server, before + committed) {
            assert_eq!(read_count(server), before + committed + 1);
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("publication did not recover after all source terminals");
}

fn publication_recovered(server: &Server, committed: u64) -> bool {
    let id = "pressure-recovered";
    let body = json!({
        "query":"mutation($id:ID!){publish(id:$id,value:12){id}}",
        "variables":{"id":id}
    })
    .to_string();
    let response = server.http("POST", &[("Content-Type", "application/json")], &body);
    if response.status != 200 {
        assert_pressure_failure(&response);
        assert_eq!(read_count(server), committed);
        return false;
    }
    let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(body["data"]["publish"]["id"], id);
    assert_eq!(read_count(server), committed + 1);
    true
}

fn assert_pressure_failure(response: &Response) {
    assert_eq!(response.status, 500);
    let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(body["data"].is_null());
    super::assertions::assert_safe_error(&body["errors"]);
}

fn read_count(server: &Server) -> u64 {
    for _ in 0..100 {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            r#"{"query":"{ count }"}"#,
        );
        if response.status == 200 {
            let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            return body["data"]["count"].as_u64().unwrap();
        }
        assert_pressure_failure(&response);
        thread::sleep(Duration::from_millis(20));
    }
    panic!("finite queries did not recover after the source terminal");
}

fn assert_ordered_events(ids: &[String]) {
    assert!(!ids.is_empty(), "source terminal must follow a whole event");
    let positions = ids.iter().map(|id| {
        let suffix = id.strip_prefix("source-").expect("source event prefix");
        let (batch, item) = suffix.split_once('-').expect("source event position");
        batch.parse::<usize>().unwrap() * 8 + item.parse::<usize>().unwrap() - 1
    });
    let mut prior = None;
    for current in positions {
        if let Some(prior) = prior {
            assert_eq!(
                current,
                prior + 1,
                "source events remain a whole FIFO suffix"
            );
        }
        prior = Some(current);
    }
}
