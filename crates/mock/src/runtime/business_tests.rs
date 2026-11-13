//! Resolver-level business failure classification does not poison the work ledger.

use chigiri::core::Object;

#[test]
fn sa_11_caught_business_failure_keeps_a_distinct_runtime_cause() {
    let _case = crate::vm_gate::test_case();
    let contract = super::tests::contract("type Mutation{fail:Int} type Query{value:Int}");
    let script = super::tests::script(
        &contract,
        "Mutation.fail",
        "pcall(function() ctx.fail({code='DENIED'}) end) return 2",
    );
    let mut store = crate::store::Store::empty(contract.ir().clone());
    let error = super::tests::run(&script, &Object::new(), &mut store)
        .err()
        .unwrap();
    assert_eq!(error.phase, "business");
}
