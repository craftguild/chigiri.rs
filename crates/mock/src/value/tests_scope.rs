//! Test aggregate capacity and failures across conversion boundaries that share one ledger.

use super::{MAX_DEPTH, MAX_NODES, ValueBudget};

#[test]
fn sd_scope_value_nodes_and_local_failures_are_sticky_across_budget_handles() {
    let mut first = ValueBudget::default();
    let mut second = ValueBudget::with_work(first.work.clone());
    for _ in 0..MAX_NODES / 2 {
        first.container(1).unwrap();
        second.container(1).unwrap();
    }
    assert_eq!(first.nodes(), MAX_NODES / 2);
    assert_eq!(second.nodes(), MAX_NODES / 2);
    assert!(first.container(1).is_err());
    assert!(second.key("a").is_err());
    let mut first = ValueBudget::default();
    let mut second = ValueBudget::with_work(first.work.clone());
    assert!(first.container(MAX_DEPTH + 1).is_err());
    assert!(second.container(1).is_err());
    assert!(ValueBudget::default().container(1).is_ok());
}
