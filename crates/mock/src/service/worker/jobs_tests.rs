//! Test cancellation-cause delivery and shutdown recovery for unfinished jobs.

use super::*;

#[test]
fn unexecuted_resolver_delivers_cancellation_or_deadline_cause() {
    for cancelled in [true, false] {
        let execution = Execution {
            state: Arc::new(Object::new()),
            request: None,
            deadline: Instant::now() + Duration::from_secs(1),
            cancel: Cancellation::default(),
            shutdown: Cancellation::default(),
            budget: Default::default(),
        };
        let budget = execution.budget.clone();
        let (reply, mut receive) = oneshot::channel();
        reject(
            Job {
                _ticket: None,
                cancel: Cancellation::default(),
                deadline: execution.deadline,
                request: None,
                work: Work::Resolve {
                    field: FieldId(usize::MAX),
                    arguments: Object::new(),
                    parent: None,
                    snapshot: None,
                    execution,
                    request: None,
                    reply,
                },
            },
            cancelled,
        );
        let result = receive.try_recv().expect("delivered job result");
        let Err(failure) = result else {
            panic!("unexecuted resolver unexpectedly succeeded");
        };
        assert_eq!(failure.cancelled, cancelled);
        assert_eq!(
            budget.0.failure(),
            Some(if cancelled {
                crate::limits::request::BudgetFailure::Cancelled
            } else {
                crate::limits::request::BudgetFailure::Deadline
            })
        );
    }
}

#[test]
fn stopped_worker_cancels_queued_jobs_and_the_queue_recovers() {
    let _case = crate::vm_gate::test_case();
    let ir = crate::service::tests::ir("type Query { value: Int! }");
    let candidates = crate::service::tests::candidates(&ir, &[("Query.value", "return 42")]);
    let scripts =
        ScriptSet::new(crate::luau::Contract::new(ir.clone()).unwrap(), candidates).unwrap();
    let (client, receiver) = super::super::queue(ir);
    let store = client.committed.clone();
    let sender = client.sender.clone();
    let mut replies = Vec::new();
    for _ in 0..crate::limits::QUEUED_JOBS {
        let (reply, receive) = oneshot::channel();
        sender
            .try_send(Job {
                _ticket: None,
                cancel: Cancellation::default(),
                deadline: Instant::now() + Duration::from_secs(1),
                request: None,
                work: Work::Snapshot(reply),
            })
            .unwrap();
        replies.push(receive);
    }
    let shutdown = client.shutdown.clone();
    shutdown.cancel();
    let consumers = super::super::consumers::ConsumerSet::prepare(
        &store,
        Vec::new(),
        super::super::consumers::ConsumerHost {
            clock: client.clock.clone(),
            mutation: client.mutation.clone(),
            logger: None,
            events: client.events.clone(),
        },
        &shutdown,
    )
    .unwrap();
    run(
        store.clone(),
        scripts,
        Vec::new(),
        receiver,
        shutdown,
        client.queue_open.clone(),
        consumers,
    );
    assert!(!*client.queue_open.lock().unwrap());
    // A producer that passed an earlier shutdown check is still rejected when enqueue resumes after draining.
    let (reply, _receive) = oneshot::channel();
    let late = client
        .enqueue(Job {
            _ticket: None,
            cancel: Cancellation::default(),
            deadline: Instant::now() + Duration::from_secs(1),
            request: None,
            work: Work::Snapshot(reply),
        })
        .unwrap_err();
    assert!(late.cancelled);
    for mut receive in replies {
        let failure = receive
            .try_recv()
            .expect("delivery to accepted job")
            .err()
            .expect("no snapshot may be returned after shutdown");
        assert!(failure.cancelled);
    }
    assert_eq!(store.lock().unwrap().snapshot().size().0, 0);
    assert!(sender.is_empty());
}

#[tokio::test]
async fn unexecuted_prepare_preserves_deadline_cause_before_cleanup_cancellation() {
    let _case = crate::vm_gate::test_case();
    let ir = crate::service::tests::ir("type Query { value: Int! }");
    for cancelled in [false, true] {
        let (client, _receiver) = super::super::queue(ir.clone());
        let execution = Execution {
            state: client.initial_state().snapshot(),
            request: None,
            deadline: Instant::now() + Duration::from_secs(1),
            cancel: Cancellation::default(),
            shutdown: client.shutdown.clone(),
            budget: Default::default(),
        };
        let client = client.request_client(true, &execution).await.unwrap();
        let request = client.request.as_ref().unwrap().clone();
        let (reply, mut receive) = oneshot::channel();
        reject(
            Job {
                _ticket: None,
                cancel: Cancellation::default(),
                deadline: execution.deadline,
                request: Some(request.clone()),
                work: Work::Prepare { request, reply },
            },
            cancelled,
        );
        assert!(execution.cancel.cancelled());
        // A sibling checkpoint after cancellation does not overwrite the original deadline failure.
        let mut sibling = execution.budget.0.clone();
        sibling.guard(&execution.cancel, &execution.shutdown, execution.deadline);
        assert!(sibling.charge(0).is_err());
        let failure = receive.try_recv().unwrap().unwrap_err();
        assert_eq!(failure.cancelled, cancelled);
        assert_eq!(
            execution.budget.failure(),
            Some(if cancelled {
                crate::limits::request::BudgetFailure::Cancelled
            } else {
                crate::limits::request::BudgetFailure::Deadline
            })
        );
        assert!(client.commit_request().is_err());
        assert_eq!(client.committed.lock().unwrap().snapshot().size().0, 0);
    }
}
