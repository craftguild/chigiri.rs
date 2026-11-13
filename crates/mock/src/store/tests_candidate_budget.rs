//! Check shared budgets and cancellation before copying candidate maps, then verify references and indexes after failure.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chigiri::core::{Object, Value};

use super::tests::{ir, model, object, row};
use super::{CallScope, Store, work::Work};
use crate::{api::WORK_LIMIT, cancellation::Cancellation};

#[test]
fn sd_scope_candidate_copy_is_prepaid_and_failure_does_not_clone_rows() {
    let ir = ir("type Item{id:ID!} type Query{x:Int}");
    let item = model(&ir, "Item");
    let store = Store::seeded(
        ir,
        &object(&[(
            "Item",
            Value::List(vec![row(&[("id", "a".into())]), row(&[("id", "b".into())])]),
        )]),
    )
    .unwrap();
    for shortage in [0, 1] {
        let mut work = Work::default();
        // Charge two state/vector entries, two row-map entries, and two identity-map entries before cloning references.
        work.charge(WORK_LIMIT - 6 + shortage).unwrap();
        let held = store.snapshot().record(item, "a").unwrap().clone();
        let refs = Arc::strong_count(&held);
        let mut reservations = 0;
        work.observe(move |charge| {
            if charge != 0 && reservations < 2 {
                assert_eq!(Arc::strong_count(&held), refs);
                reservations += 1;
            }
        });
        let result = store.begin_with_work(CallScope::MutationRoot, work.clone());
        if shortage == 0 {
            let tx = result.unwrap();
            assert!(work.charge(1).is_err());
            assert!(tx.prepare().is_err());
        } else {
            assert!(result.is_err());
            assert!(work.check().is_err());
        }
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    let mut work = Work::default();
    work.guard(
        &cancel,
        &Cancellation::default(),
        Instant::now() + Duration::from_secs(1),
    );
    assert!(
        store
            .begin_with_work(CallScope::MutationRoot, work.clone())
            .is_err()
    );
    assert!(work.check().is_err());
    let mut next = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(
        next.create(item, &object(&[("id", "c".into())]))
            .unwrap()
            .row(),
        2
    );
    next.prepare().unwrap();
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn sd_scope_nullable_storage_map_is_reserved_before_value_validation() {
    let fields = (0..500)
        .map(|i| format!("f{i}:String "))
        .collect::<String>();
    let ir = ir(&format!("type Wide{{{fields}}} type Query{{x:Int}}"));
    let wide = model(&ir, "Wide");
    let mut store = Store::empty(ir);
    let mut work = Work::default();
    work.charge(WORK_LIMIT - 501).unwrap();
    let mut tx = store
        .begin_with_work(CallScope::MutationRoot, work)
        .unwrap();
    assert!(tx.create(wide, &Object::new()).is_err());
    assert_eq!(
        tx.values.nodes(),
        0,
        "map growth must not begin before reservation"
    );
    assert!(tx.prepare().is_err());
    let mut next = store.begin(CallScope::MutationRoot).unwrap();
    let row = next.create(wide, &Object::new()).unwrap();
    assert_eq!(row.row(), 0);
    assert_eq!(row.values().len(), 500);
    assert!(row.values().all(|(_, value)| value.is_null()));
    store.commit(next.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().size().0, 1);
}
