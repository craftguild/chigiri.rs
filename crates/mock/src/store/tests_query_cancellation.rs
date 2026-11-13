//! Cancel count and sort work from another thread, rejecting partial results and candidate commits.

use crate::store::test_selection::id as select;

use super::tests::{ir, model, object, row};
use super::tests_query::order;
use super::{CallScope, Store};
use crate::cancellation::Cancellation;
use chigiri::core::{Object, Value};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn mr_09_13_15_cancel_during_rust_scan_and_sort_rejects_partial_result_and_recovers() {
    let ir = ir("type Item{id:ID! rank:Int!} type Query{x:Int}");
    let model = model(&ir, "Item");
    let records = (0..500)
        .map(|index| {
            row(&[
                ("id", format!("seed-{index}").into()),
                ("rank", Value::Int(500 - index)),
            ])
        })
        .collect();
    let mut store = Store::seeded(ir, &object(&[("Item", Value::List(records))])).unwrap();
    for sort in [false, true] {
        for stop_worker in [false, true] {
            let old = store.snapshot();
            let cancel = Cancellation::default();
            let shutdown = Cancellation::default();
            let mut tx = store.begin(CallScope::MutationRoot).unwrap();
            let created = tx
                .create(
                    model,
                    &object(&[("id", "Item-1".into()), ("rank", 999.into())]),
                )
                .unwrap();
            assert_eq!(created.id().unwrap(), "Item-1");
            assert_eq!(created.row(), 500);
            tx.execution_guard(&cancel, &shutdown, Instant::now() + Duration::from_secs(10));
            let (started, progress) = mpsc::channel();
            let (resume, released) = mpsc::channel();
            let mut calls = 0;
            let mut sorting = false;
            let mut stopped = false;
            tx.work.observe(move |charge| {
                // Sorting reserves scratch space for all 501 record handles before performing repeated comparisons.
                if sort && charge == 501 {
                    sorting = true;
                    calls = 0;
                }
                if !sort || sorting {
                    calls += 1;
                }
                if !stopped && calls == 100 {
                    stopped = true;
                    started.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(3)).unwrap();
                }
            });
            let worker = std::thread::spawn(move || {
                let failed = if sort {
                    tx.find(
                        model,
                        &object(&[("orderBy", Value::List(vec![order("rank", "asc")]))]),
                    )
                    .is_err()
                } else {
                    tx.count(model, &Object::new()).is_err()
                };
                (failed, tx)
            });
            progress.recv_timeout(Duration::from_secs(3)).unwrap();
            if stop_worker {
                shutdown.cancel();
            } else {
                cancel.cancel();
            }
            let stopped_at = Instant::now();
            resume.send(()).unwrap();
            let (failed, tx) = worker.join().unwrap();
            assert!(failed, "cancellation must not return a partial result");
            assert!(stopped_at.elapsed() < Duration::from_secs(1));
            assert!(tx.prepare().is_err());
            assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 500);
            assert_eq!(old.count(model, &Object::new()).unwrap(), 500);
            let rows = store
                .snapshot()
                .find(
                    model,
                    &object(&[
                        ("orderBy", Value::List(vec![order("rank", "asc")])),
                        ("limit", 1.into()),
                    ]),
                )
                .unwrap();
            assert_eq!(rows[0].id().unwrap(), "seed-499");
            let mut recovery = store.begin(CallScope::MutationRoot).unwrap();
            assert_eq!(
                recovery
                    .create(
                        model,
                        &object(&[("id", "Item-1".into()), ("rank", 7.into())])
                    )
                    .unwrap()
                    .id()
                    .unwrap(),
                "Item-1"
            );
            recovery.delete(model, &select("Item-1")).unwrap();
            // Verify the successful candidate without changing the next case's baseline.
            recovery.prepare().unwrap();
        }
    }
    let mut final_tx = store.begin(CallScope::MutationRoot).unwrap();
    final_tx
        .create(
            model,
            &object(&[("id", "Item-1".into()), ("rank", 7.into())]),
        )
        .unwrap();
    store.commit(final_tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 501);
}
