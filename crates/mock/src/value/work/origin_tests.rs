//! Preserve the triggering operation's origin when parallel handles share one ledger.

use super::*;

fn origin(name: &'static str) -> LimitOrigin {
    LimitOrigin {
        phase: "field.after",
        coordinate: Some(format!("Query.{name}").into()),
        path: vec![chigiri::PathSegment::Field(name.into())].into(),
    }
}

#[test]
fn sibling_handle_origin_is_not_overwritten_by_delivery_or_later_work() {
    let mut work = Work::default();
    work.charge(policy(LimitKind::Work).soft - 2).unwrap();
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let received = warnings.clone();
    work.with_warnings(move |warning| received.lock().unwrap().push(warning));
    let mut first = work.at(origin("first"));
    let mut second = work.at(origin("second"));
    let (send, receive) = std::sync::mpsc::channel();
    let first_thread = std::thread::spawn(move || {
        first.charge(1).unwrap();
        send.send(()).unwrap();
        first
    });
    let second_thread = std::thread::spawn(move || {
        receive.recv().unwrap();
        second.charge(1).unwrap();
    });
    let mut first = first_thread.join().unwrap();
    second_thread.join().unwrap();
    first.charge(1).unwrap();
    let warnings = warnings.lock().unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].origin.phase, "field.after");
    assert_eq!(
        warnings[0].origin.coordinate.as_deref(),
        Some("Query.second")
    );
    assert_eq!(
        warnings[0].origin.path.as_ref(),
        &[chigiri::PathSegment::Field("second".into())]
    );
    assert_eq!(warnings[0].observed, policy(LimitKind::Work).soft);
    assert!(work.failure().is_none());
}

#[test]
fn pending_warning_keeps_the_origin_that_crossed_the_threshold() {
    let work = Work::default();
    work.at(origin("first"))
        .charge(policy(LimitKind::Work).soft)
        .unwrap();
    work.at(origin("second")).charge(1).unwrap();
    let warning = work.ledger.lock().unwrap().pending[LimitKind::Work as usize]
        .take()
        .unwrap();
    assert_eq!(warning.origin.coordinate.as_deref(), Some("Query.first"));
}
