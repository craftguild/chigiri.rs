//! General script/host callback-lifetime regression coverage.

use super::*;
use chigiri::core::Value;

#[test]
fn rsm_15_17_root_vm_releases_all_callback_captures_after_each_run() {
    let _test_case = crate::vm_gate::test_case();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let contract = tests::contract(include_str!("../../fixtures/blog.graphql"));
            let script = tests::script(
                &contract,
                "Query.getUsers",
                "return ctx.models.User:find({limit = args.limit})",
            );
            let state = Rc::new(RefCell::new(Host {
                ir: contract.ir().clone(),
                session: Session::Read(Store::empty(contract.ir().clone()).snapshot().reader()),
                failed: false,
                cancelled_failure: false,
                cancel_unwinding: false,
                last_line: 0,
                stack: None,
                stack_captures: 0,
                stack_source: script.coordinate().into(),
                calls: 0,
                cancel: Cancellation::default(),
                shutdown: Cancellation::default(),
                deadline: Instant::now() + VM_TIME,
                request_state: Arc::new(Object::new()),
                request: None,
                clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system())
                    .capture(),
                business: Default::default(),
                business_path: Default::default(),
                business_unwinding: false,
                publications: None,
                hook: None,
            }));
            let arguments = tests::args(&[("limit", Value::Int(20))]);
            for _ in 0..10 {
                state.borrow_mut().deadline = Instant::now() + VM_TIME;
                assert_eq!(
                    vm::run(&script, &arguments, None, &state).unwrap(),
                    Value::List(Vec::new())
                );
                assert_eq!(
                    Rc::strong_count(&state),
                    1,
                    "the VM and callback still form a strong reference cycle"
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
