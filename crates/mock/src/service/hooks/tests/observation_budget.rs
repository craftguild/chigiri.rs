//! Verify a shared hard failure stops every candidate and later phase, then resets for the next request.

use super::*;
use HookSlot::*;
use serde_json::json;

#[tokio::test]
async fn ra_after_exhaustion_rolls_back_all_roots_and_next_request_recovers() {
    let _case = crate::vm_gate::test_case();
    let seed = json!({"A":[{"words":vec![""; 1000]}]}).to_string();
    // It increases the stack of type testing/reform fixtures and maintains the VM limitations of the executable worker.
    let fixture = std::thread::Builder::new().stack_size(8 * 1024 * 1024).spawn(move || Fixture::new(
        "type A{words:[String!]!} type B{id:ID!} type Query{bs:[B!]!} type Mutation{make(id:ID!):B!}",
        &seed,
        &[("Mutation.make", "return ctx.models.B:create({id=args.id})"),
          ("Query.bs", "return ctx.models.B:find({})")],
        &[(RequestBefore, "return nil"),
          (OperationAfter, "ctx.log('info', 'outer', {}) return nil"),
          (RequestAfter, "ctx.log('info', 'outer', {}) return nil"), (FieldAfter, r#"
            ctx.log('info', 'field-observed', {})
            if ctx.field.coordinate == 'Mutation.make' and ctx.field.path[1] == 'first' then
                ctx.log('info', 'entered', {})
                for _=1,100 do pcall(function() ctx.models.A:find({}) end) end
                ctx.log('info', 'unexpected completion', {})
            end
            return nil
        "#)],
    )).unwrap().join().unwrap();
    let (response, _) = fixture
        .request(
            "POST",
            None,
            br#"{"query":"mutation{first:make(id:\"one\"){id} second:make(id:\"two\"){id}}"}"#
                .to_vec(),
            &[],
        )
        .await;
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], json!(null));
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    {
        let events = fixture.events.lock().unwrap();
        assert!(events.iter().any(|event| {
            event
                .record
                .as_ref()
                .is_ok_and(|record| record.message == "entered")
        }));
        let failures = events
            .iter()
            .filter(|event| event.record.is_err())
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].slot, FieldAfter);
        assert_eq!(failures[0].coordinate.as_deref(), Some("Mutation.make"));
        assert_eq!(
            failures[0].path.as_ref(),
            [chigiri::PathSegment::from("first")]
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event
                    .record
                    .as_ref()
                    .is_ok_and(|record| record.message == "field-observed"))
                .count(),
            1
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.slot, OperationAfter | RequestAfter))
        );
        assert!(!events.iter().any(|event| {
            event
                .record
                .as_ref()
                .is_ok_and(|record| record.message == "unexpected completion")
        }));
    }
    fixture.events.lock().unwrap().clear();
    assert_eq!(
        fixture.query("{bs{id}}").await,
        (json!({"data":{"bs":[]}}), None)
    );
    {
        let events = fixture.events.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.record.is_ok()));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.slot == FieldAfter)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.slot == OperationAfter)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.slot == RequestAfter)
                .count(),
            1
        );
    }
    fixture.stop().await;
}

#[tokio::test]
async fn sd_first_observation_conversion_failure_is_reported_once_at_its_field() {
    use super::scheduling::{Pause, Schedule, Stage};
    let _case = crate::vm_gate::test_case();
    let pause = Pause::new("first", Stage::AfterResolver);
    let fixture = Fixture::new(
        "type B{id:ID!} type Query{bs:[B!]!} type Mutation{make(id:ID!):B!}",
        "{}",
        &[
            ("Mutation.make", "return ctx.models.B:create({id=args.id})"),
            ("Query.bs", "return ctx.models.B:find({})"),
        ],
        &[
            (FieldAfter, "ctx.log('info', 'after', {}) return nil"),
            (OperationAfter, "ctx.log('info', 'after', {}) return nil"),
            (RequestAfter, "ctx.log('info', 'after', {}) return nil"),
        ],
    )
    .scheduled(Schedule::new(std::slice::from_ref(&pause)));
    let events = fixture.events.clone();
    let scope = HttpScope::new("127.0.0.1:4000".parse().unwrap())
        .with_events(move |event| events.lock().unwrap().push(event));
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let (response, ()) = tokio::join!(
        scope.run(fixture.endpoint.handle(request, BufferedBody::new(
            br#"{"query":"mutation{first:make(id:\"one\"){id} second:make(id:\"two\"){id}}"}"#.to_vec()), &Clock)),
        async {
            pause.reached().await;
            // Exhaust the shared request ledger after the first candidate/field observation but before result conversion.
            assert!(scope.0.observations().lock().unwrap().work.charge(usize::MAX).is_err());
            pause.proceed().await;
        }
    );
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], json!(null));
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(value["errors"][0]["extensions"]["limit"], "work");
    {
        let events = fixture.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].record.is_err());
        assert_eq!(events[0].slot, FieldAfter);
        assert_eq!(events[0].coordinate.as_deref(), Some("Mutation.make"));
        assert_eq!(
            events[0].path.as_ref(),
            [chigiri::PathSegment::from("first")]
        );
    }
    fixture.events.lock().unwrap().clear();
    assert_eq!(
        fixture.query("{bs{id}}").await,
        (json!({"data":{"bs":[]}}), None)
    );
    assert_eq!(fixture.events.lock().unwrap().len(), 3);
    assert!(
        fixture
            .events
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.record.is_ok())
    );
    fixture.stop().await;
}
