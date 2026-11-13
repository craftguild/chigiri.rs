//! A queued sibling must not manufacture another failure after its request stops.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::Object;
use tokio::sync::oneshot;

use super::super::{Job, Work, execute_job};
use crate::cancellation::Cancellation;
use crate::ir::FieldId;
use crate::limits::request::{BudgetFailure, LimitKind, LimitNotice, LocalLimit};
use crate::luau::ScriptSet;
use crate::runtime::Execution;

#[test]
fn queued_resolver_observes_linked_cancellation_before_contract_or_native_validation() {
    let _case = crate::vm_gate::test_case();
    let ir = crate::service::tests::ir("type Query { value: Int! }");
    let scripts = ScriptSet::new(
        crate::luau::Contract::new(ir.clone()).unwrap(),
        crate::luau::static_candidates(&ir).unwrap(),
    )
    .unwrap();
    let (client, _receiver) = super::super::super::queue(ir);
    for cause in [
        BudgetFailure::Execution,
        BudgetFailure::Deadline,
        BudgetFailure::Hard(LimitNotice {
            kind: LimitKind::Work,
            observed: 1_000_001,
        }),
        BudgetFailure::LocalHard(LocalLimit::HostCalls),
        BudgetFailure::Cancelled,
    ] {
        let parent = Cancellation::default();
        let job_cancel = Cancellation::default();
        let execution = Execution {
            state: Arc::new(Object::new()),
            request: None,
            deadline: Instant::now() + Duration::from_secs(1),
            cancel: job_cancel.linked(&parent),
            shutdown: client.shutdown.clone(),
            budget: Default::default(),
        };
        let budget = execution.budget.clone();
        budget.0.clone().fail_with(cause);
        parent.cancel();
        assert!(!job_cancel.cancelled());
        let (reply, mut receive) = oneshot::channel();
        execute_job(
            &client.committed,
            &scripts,
            &[],
            Job {
                cancel: job_cancel,
                deadline: execution.deadline,
                request: None,
                _ticket: None,
                work: Work::Resolve {
                    // Running this field would create an independent contract error.
                    field: FieldId(usize::MAX),
                    arguments: Object::new(),
                    parent: None,
                    snapshot: None,
                    execution,
                    request: None,
                    reply,
                },
            },
            &client.shutdown,
        );
        let error = receive
            .try_recv()
            .unwrap()
            .err()
            .expect("cancel before dispatch");
        assert!(error.cancelled, "{cause:?}");
        assert_eq!(budget.failure(), Some(cause));
    }
}
