//! Share regex work across schema defaults and one complete seed pass without leaking it to later startups.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::Store;
use crate::ir::MockIr;

#[test]
fn sd_14_default_and_seed_share_match_count_without_repeated_startup_accumulation() {
    let defaults = vec!["\"\""; 100].join(",");
    let source = format!(
        "type Item{{words:[String!]! @constraint(pattern:\"\")}} type Query{{x(words:[String!] = [{defaults}] @constraint(pattern:\"\")):Int}}"
    );
    let ir = Arc::new(MockIr::parse(vec![Source::new("scope.graphql", source)]).unwrap());
    let row = |count| {
        let mut row = Object::new();
        row.insert("words", Value::List(vec![Value::from(""); count]));
        Value::Object(row)
    };
    let mut exact = Object::new();
    exact.insert("Item", Value::List(vec![row(999); 4]));
    assert_eq!(
        Store::seeded(ir.clone(), &exact)
            .unwrap()
            .snapshot()
            .size()
            .0,
        4
    );
    assert!(Store::seeded(ir.clone(), &exact).is_ok());
    let mut over = Object::new();
    over.insert(
        "Item",
        Value::List(vec![row(999), row(999), row(999), row(1000)]),
    );
    let error = Store::seeded(ir, &over).err().unwrap();
    assert_eq!(error.0, "request aggregate limit");
}
