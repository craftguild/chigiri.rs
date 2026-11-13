//! Verify queue reservation, bounded JSON encoding, terminal records, and confirmed delivery.

use std::sync::Arc;

use chigiri::PathSegment;
use serde_json::json;

use super::*;
use crate::hooks::HookSlot;
use crate::runtime::HookLog;
use crate::service::HookEvent;

#[path = "tests/lifecycle.rs"]
mod subscription_lifecycle;
mod subscriptions;

#[test]
fn soft_warning_stderr_is_framed_and_full_or_closed_output_counts_loss() {
    use crate::limits::request::{LimitKind, policy};
    let (logger, [hooks, accesses, _, notices]) = queues();
    let policy = policy(LimitKind::Work);
    let notice = crate::limits::warning::LimitWarning {
        origin: Default::default(),
        kind: LimitKind::Work,
        observed: policy.soft,
    };
    logger.limit_warning(42, notice.clone());
    let record = notices.try_recv().unwrap();
    assert!(matches!(record.kind, Kind::Notice));
    let parsed: serde_json::Value = serde_json::from_slice(&record.bytes).unwrap();
    assert_eq!(parsed["kind"], "limit_warning");
    assert_eq!(parsed["level"], "warn");
    assert_eq!(parsed["request_id"], 42);
    assert_eq!(parsed["limit"], policy.name);
    assert_eq!(parsed["soft"], policy.soft);
    assert_eq!(parsed["hard"], policy.hard);
    assert_eq!(parsed["observed"], policy.soft);
    assert!(hooks.is_empty() && accesses.is_empty());
    for _ in 0..NOTICE_RECORDS + 1 {
        logger.limit_warning(42, notice.clone());
    }
    assert_eq!(logger.loss().notice, 1);
    drop(notices);
    logger.limit_warning(42, notice.clone());
    assert_eq!(logger.loss().notice, 2);
}

fn queues() -> (Logger, [crossbeam_channel::Receiver<Record>; 4]) {
    let (hook, hooks) = bounded(QUEUE_RECORDS);
    let (access, accesses) = bounded(QUEUE_RECORDS);
    let (subscription, subscriptions) = bounded(QUEUE_RECORDS);
    let (notice, notices) = bounded(NOTICE_RECORDS);
    (
        Logger {
            hook,
            access,
            subscription,
            notice,
            counts: Arc::default(),
            stopping: Arc::default(),
        },
        [hooks, accesses, subscriptions, notices],
    )
}
fn event(slot: HookSlot) -> HookEvent {
    let mut fields = chigiri::core::Object::new();
    fields.insert("kind", "spoof");
    fields.insert("request_id", 999_i32);
    HookEvent {
        request_id: 7,
        slot,
        module: Some(slot.path().into()),
        coordinate: Some(Arc::from("Query.value")),
        path: Arc::from([PathSegment::from("alias"), PathSegment::from(0_usize)]),
        record: Ok(HookLog {
            level: "info",
            message: "first\nsecond\u{1b}]0;title\u{7}".into(),
            fields,
        }),
    }
}
#[test]
fn hk_15_json_envelope_all_slots_escape_control_and_keep_fields_separate() {
    for slot in HookSlot::ALL {
        let event = event(slot);
        let bytes = records::hook(&event).unwrap();
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(!bytes.contains(&27));
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["format"], FORMAT);
        assert_eq!(parsed["kind"], "hook");
        assert_eq!(parsed["request_id"], 7);
        assert_eq!(parsed["fields"], json!({"kind":"spoof","request_id":999}));
        assert_eq!(parsed["phase"], serde_json::to_value(slot.phase()).unwrap());
        assert_eq!(
            parsed["timing"],
            serde_json::to_value(slot.timing()).unwrap()
        );
        assert!(parsed["timestamp"].as_i64().unwrap() > 0);
        assert_eq!(parsed["message"], event.record.unwrap().message);
        if slot.phase() == crate::hooks::HookPhase::Field {
            assert_eq!(parsed["path"], json!(["alias", 0]));
        }
    }
}
#[test]
fn hk_15_17_18_reserved_access_queue_and_oversize_loss_never_fail_producer() {
    let (logger, [hooks, accesses, _, _]) = queues();
    for _ in 0..QUEUE_RECORDS + 20 {
        logger.hook(event(HookSlot::RequestBefore));
    }
    assert_eq!(hooks.len(), QUEUE_RECORDS);
    assert_eq!(logger.loss().hook, 20);
    let mut access = Access::new(logger.clone(), 8, "POST", "/graphql");
    access.response(403);
    access.complete();
    drop(access);
    assert_eq!(accesses.len(), 1);
    assert_eq!(logger.loss().access, 0);
    let oversized = HookEvent {
        record: Ok(HookLog {
            level: "info",
            message: "\n".repeat(RECORD_BYTES),
            fields: Default::default(),
        }),
        ..event(HookSlot::RequestBefore)
    };
    logger.hook(oversized);
    assert_eq!(logger.loss().hook, 21);
    let mut diagnostic = event(HookSlot::FieldAfter);
    diagnostic.record = Err(crate::luau::ScriptError::new(
        "runtime",
        "PRIVATE_SOURCE_OR_PROVIDER",
    ));
    let text = String::from_utf8(records::hook(&diagnostic).unwrap()).unwrap();
    assert!(!text.contains("PRIVATE_SOURCE_OR_PROVIDER"));
    assert!(text.contains("Mock hook failed"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["fields"], json!({}));
    assert_eq!(value["diagnostic"]["module"], HookSlot::FieldAfter.path());
    assert_eq!(value["diagnostic"]["line"], serde_json::Value::Null);
    assert_eq!(value["diagnostic"]["column"], serde_json::Value::Null);
    assert_eq!(value["diagnostic"]["member"], serde_json::Value::Null);
    assert_eq!(value["diagnostic"]["expected"], "valid contract");
    assert_eq!(value["diagnostic"]["actual"], "contract violation");
}

#[test]
fn ab09_15_log_v4_diagnostic_fixtures_are_exact_and_value_free() {
    let resolver = crate::service::ResolverDiagnostic {
        stack: None,
        request_id: 41,
        coordinate: "Image.url".into(),
        module: None,
        phase: "output",
        cause: "MOCK_OUTPUT_TYPE_MISMATCH",
        member: Some("Image.url".into()),
        expected: Some("String".into()),
        actual: Some("number".into()),
        hint: Some("Return a value matching the field's GraphQL type.".into()),
        line: None,
        column: None,
        cancelled: false,
    };
    let bytes = records::resolver(&resolver).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        json!({
            "format": "chigiri-mock-log/v5",
            "kind": "resolver_diagnostic",
            "stack": null,
            "request_id": 41,
            "coordinate": "Image.url",
            "module": null,
            "phase": "output",
            "cause": "MOCK_OUTPUT_TYPE_MISMATCH",
            "member": "Image.url",
            "expected": "String",
            "actual": "number",
            "hint": "Return a value matching the field's GraphQL type.",
            "line": null,
            "column": null,
            "cancelled": false
        })
    );

    let mut hook = event(HookSlot::FieldAfter);
    hook.record = Err(crate::luau::ScriptError::new("runtime", "PRIVATE_TEXT"));
    let hook_bytes = records::hook(&hook).unwrap();
    let mut actual: serde_json::Value = serde_json::from_slice(&hook_bytes).unwrap();
    actual.as_object_mut().unwrap().remove("timestamp");
    assert_eq!(
        actual,
        json!({
            "format": "chigiri-mock-log/v5",
            "request_id": 7,
            "kind": "hook",
            "phase": "field",
            "timing": "after",
            "level": "error",
            "message": "Mock hook failed",
            "coordinate": "Query.value",
            "path": ["alias", 0],
            "fields": {},
            "diagnostic": {
                "stack": null,
                "module": "hooks/field/after.luau",
                "phase": "runtime",
                "cause": "MOCK_CONTRACT",
                "member": null,
                "expected": "valid contract",
                "actual": "contract violation",
                "hint": "Check the generated contract.",
                "line": null,
                "column": null
            }
        })
    );
    assert!(
        !String::from_utf8(hook_bytes)
            .unwrap()
            .contains("PRIVATE_TEXT")
    );
}

#[test]
fn hk_16_access_records_completed_and_both_cancel_positions_once() {
    let (logger, [_, accesses, _, _]) = queues();
    {
        let _cancelled = Access::new(logger.clone(), 1, "POST", "/graphql");
    }
    {
        let mut cancelled = Access::new(logger.clone(), 2, "GET", "/graphiql");
        cancelled.response(200);
    }
    {
        let mut completed = Access::new(logger, 3, "OPTIONS", "/graphql");
        completed.response(204);
        completed.complete();
        completed.complete();
    }
    let values: Vec<serde_json::Value> = accesses
        .try_iter()
        .map(|record| serde_json::from_slice(&record.bytes).unwrap())
        .collect();
    assert_eq!(values.len(), 3);
    assert_eq!(values[0]["status"], json!(null));
    assert_eq!(values[0]["outcome"], "cancelled");
    assert_eq!(values[1]["status"], 200);
    assert_eq!(values[1]["outcome"], "cancelled");
    assert_eq!(values[2]["status"], 204);
    assert_eq!(values[2]["outcome"], "completed");
    assert!(
        values
            .iter()
            .all(|value| value["duration_ms"].as_f64().unwrap() >= 0.0)
    );
}
#[test]
fn hk_15_17_internal_writer_acknowledges_only_complete_flush_and_bounds_frames() {
    use std::io::{self, Write};
    fn frame(text: &[u8]) -> Vec<u8> {
        let mut bytes = (text.len() as u32).to_be_bytes().to_vec();
        bytes.extend(text);
        bytes
    }
    let mut input = frame(b"one\n");
    input.extend(frame(b"two\n"));
    let (mut output, mut ack) = (Vec::new(), Vec::new());
    serve_writer(input.as_slice(), &mut output, &mut ack).unwrap();
    assert_eq!(output, b"one\ntwo\n");
    assert_eq!(ack, b"11");
    struct FailOnce(bool);
    impl Write for FailOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0 {
                self.0 = false;
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    ack.clear();
    serve_writer(input.as_slice(), FailOnce(true), &mut ack).unwrap();
    assert_eq!(ack, b"01");
    struct Uncertain {
        bytes: Vec<u8>,
        partial: bool,
    }
    impl Write for Uncertain {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.partial && !self.bytes.is_empty() {
                return Err(io::ErrorKind::StorageFull.into());
            }
            let length = if self.partial { 1 } else { bytes.len() };
            self.bytes.extend(&bytes[..length]);
            Ok(length)
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    for partial in [false, true] {
        let mut sink = Uncertain {
            bytes: Vec::new(),
            partial,
        };
        ack.clear();
        serve_writer(input.as_slice(), &mut sink, &mut ack).unwrap();
        assert_eq!(ack, b"2");
        assert_eq!(sink.bytes, if partial { b"o".as_slice() } else { b"one\n" });
    }
    for invalid in [
        ((RECORD_BYTES + 1) as u32).to_be_bytes().to_vec(),
        frame(b"two\nlines\n"),
        frame(b"\xff\n"),
        frame(b"\x1b[2J\n"),
    ] {
        assert!(serve_writer(invalid.as_slice(), Vec::new(), Vec::new()).is_err());
    }
}

#[test]
fn hk_17_shutdown_racing_producers_counts_every_unqueued_record() {
    let (logger, [hooks, _, _, _]) = queues();
    let start = std::sync::Barrier::new(5);
    let removed = std::thread::scope(|scope| {
        for _ in 0..4 {
            let (logger, start) = (&logger, &start);
            scope.spawn(move || {
                start.wait();
                for _ in 0..100 {
                    logger.hook(event(HookSlot::RequestBefore));
                }
            });
        }
        start.wait();
        logger.stopping.lock().unwrap().close = true;
        hooks.try_iter().take(QUEUE_RECORDS).count()
    });
    assert!(hooks.is_empty());
    assert_eq!(removed as u64 + logger.loss().hook, 400);
}
