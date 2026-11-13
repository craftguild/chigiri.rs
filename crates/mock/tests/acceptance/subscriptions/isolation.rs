//! Prove registration non-replay and per-subscriber topic/selection isolation.

use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::assertions::Profile;
use super::{http, publish_to, websocket};
use crate::support::Server;

const SHARED_TOPIC: &str = "isolation-shared";
const ISOLATED_TOPIC: &str = "isolation-websocket";
const MAX_WARMUPS: usize = 8;

const SSE: &str = r#"subscription { changed(topic: "isolation-shared") { id } }"#;
const MULTIPART: &str = r#"subscription {
  changed(topic: "isolation-shared") { id value related { label } }
}"#;
const WEBSOCKET: &str = r#"subscription { changed(topic: "isolation-websocket") { id value } }"#;

pub(super) fn exercise(server: &Server) {
    publish_to(server, SHARED_TOPIC, "isolation-pre-shared", -1);
    publish_to(server, ISOLATED_TOPIC, "isolation-pre-websocket", -2);

    let (ready_tx, ready_rx) = mpsc::channel();
    let (active_tx, active_rx) = mpsc::channel();
    let clients = [
        spawn(
            server,
            Profile::ServerSentEvents,
            ready_tx.clone(),
            active_tx.clone(),
        ),
        spawn(
            server,
            Profile::Multipart,
            ready_tx.clone(),
            active_tx.clone(),
        ),
        spawn(server, Profile::WebSocket, ready_tx, active_tx),
    ];
    for _ in 0..3 {
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("each isolated protocol starts");
    }

    let mut active = [false; 3];
    for attempt in 0..MAX_WARMUPS {
        if !active[Profile::ServerSentEvents.index()] || !active[Profile::Multipart.index()] {
            publish_to(
                server,
                SHARED_TOPIC,
                &format!("isolation-warmup-shared-{attempt}"),
                i64::try_from(attempt).unwrap(),
            );
        }
        if !active[Profile::WebSocket.index()] {
            publish_to(
                server,
                ISOLATED_TOPIC,
                &format!("isolation-warmup-websocket-{attempt}"),
                i64::try_from(attempt).unwrap(),
            );
        }
        while let Ok(profile) = active_rx.recv_timeout(Duration::from_millis(250)) {
            active[profile.index()] = true;
            if active.iter().all(|seen| *seen) {
                break;
            }
        }
        if active.iter().all(|seen| *seen) {
            break;
        }
    }
    assert!(
        active.iter().all(|seen| *seen),
        "all isolated sources register"
    );

    publish_to(server, SHARED_TOPIC, "isolation-final-shared", 31);
    publish_to(server, ISOLATED_TOPIC, "isolation-final-websocket", 32);
    for client in clients {
        client.join().expect("bounded isolated wire client");
    }
}

fn spawn(
    server: &Server,
    profile: Profile,
    ready: Sender<Profile>,
    active: Sender<Profile>,
) -> thread::JoinHandle<()> {
    let address = server.address;
    thread::spawn(move || match profile {
        Profile::ServerSentEvents | Profile::Multipart => {
            let document = if profile == Profile::ServerSentEvents {
                SSE
            } else {
                MULTIPART
            };
            let mut stream = http::hold(address, profile, document);
            ready.send(profile).unwrap();
            consume(profile, || stream.next_changed(), active);
        }
        Profile::WebSocket => {
            let mut stream = websocket::hold(address, WEBSOCKET);
            ready.send(profile).unwrap();
            consume(profile, || stream.next_changed(), active);
            stream.complete();
        }
    })
}

fn consume(profile: Profile, mut next: impl FnMut() -> Value, active: Sender<Profile>) {
    let first = next();
    let first_id = id(&first);
    assert!(
        first_id.starts_with(warmup_prefix(profile)),
        "{profile:?} replayed or leaked {first_id}"
    );
    assert_shape(profile, &first, first_id);
    active.send(profile).unwrap();

    for _ in 0..=MAX_WARMUPS {
        let event = next();
        let current = id(&event);
        assert!(
            current.starts_with(warmup_prefix(profile)) || current == final_id(profile),
            "{profile:?} replayed or leaked {current}"
        );
        assert_shape(profile, &event, current);
        if current == final_id(profile) {
            return;
        }
    }
    panic!("{profile:?} final event exceeded bounded warm-up prefix")
}

fn id(event: &Value) -> &str {
    event["id"].as_str().expect("selected event ID")
}

fn warmup_prefix(profile: Profile) -> &'static str {
    match profile {
        Profile::ServerSentEvents | Profile::Multipart => "isolation-warmup-shared-",
        Profile::WebSocket => "isolation-warmup-websocket-",
    }
}

fn final_id(profile: Profile) -> &'static str {
    match profile {
        Profile::ServerSentEvents | Profile::Multipart => "isolation-final-shared",
        Profile::WebSocket => "isolation-final-websocket",
    }
}

fn assert_shape(profile: Profile, event: &Value, event_id: &str) {
    let suffix = event_id.rsplit('-').next().unwrap();
    let value = match event_id {
        "isolation-final-shared" => 31,
        "isolation-final-websocket" => 32,
        _ => suffix.parse::<i64>().expect("warm-up sequence"),
    };
    let expected = match profile {
        Profile::ServerSentEvents => json!({"id":event_id}),
        Profile::Multipart => json!({
            "id":event_id,
            "value":value,
            "related":{"label":format!("current-{event_id}")}
        }),
        Profile::WebSocket => json!({"id":event_id,"value":value}),
    };
    assert_eq!(*event, expected, "{profile:?} exact projected selection");
}
