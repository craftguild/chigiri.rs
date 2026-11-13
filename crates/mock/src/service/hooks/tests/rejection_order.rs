//! Verify response order and rejection precedence when a sibling field never starts.

use super::scheduling::{Pause, Schedule, Stage};
use super::*;
use HookSlot::*;
use serde_json::json;

#[tokio::test]
async fn dropping_a_sibling_after_rejection_preserves_its_vm_error() {
    let _case = crate::vm_gate::test_case();
    for second in [500, 499, 422, 415] {
        let before = [
            Pause::new("a", Stage::BeforeHook),
            Pause::new("b", Stage::BeforeHook),
        ];
        let ready = [
            Pause::new("a", Stage::HookResult),
            Pause::new("b", Stage::HookResult),
        ];
        let after = Pause::new("a", Stage::AfterHook);
        let body = r#"
            local code = ctx.field.arguments.requested
            if code == 200 then ctx.reject({code=200}) end
            if code == 499 then ctx.reject({code=499}) end
            if code == 422 then
                local invalid: any = {statePatch={count=99}} return invalid
            end
            if code == 415 then
                local level: any = 'invalid'
                pcall(function() ctx.log(level,'private host failure') end)
                return nil
            end
            error('private sibling failure')
        "#;
        let fixture = Fixture::new(
            "type Query { value(requested:Int!): Int! @chigiri_mock_resolver }",
            "{}",
            &[("Query.value", "error('resolver must not run')")],
            &[(FieldBefore, body)],
        )
        .scheduled(Schedule::new(&[
            before[0].clone(),
            before[1].clone(),
            ready[0].clone(),
            ready[1].clone(),
            after.clone(),
        ]));
        let query = format!("{{a:value(requested:200) b:value(requested:{second})}}");
        let (response, ()) = tokio::join!(
            fixture.request(
                "POST",
                None,
                serde_json::to_vec(&json!({"query":query})).unwrap(),
                &[]
            ),
            async {
                before[0].reached().await;
                before[1].reached().await;
                for index in [0, 1] {
                    before[index].proceed().await;
                    ready[index].reached().await;
                }
                // Both hook results arrive, but non-null propagation for `a` drops `b`'s outer field future.
                ready[0].proceed().await;
                after.reached().await;
                after.proceed().await;
            }
        );
        assert_eq!(response.0.status, 500);
        let value: serde_json::Value = serde_json::from_slice(&response.0.body).unwrap();
        assert_eq!(value["data"], json!(null));
        assert_eq!(
            value["errors"][0]["extensions"]["code"],
            "MOCK_EXECUTION_FAILED"
        );
        assert!(!value.to_string().contains("private sibling failure"));
        {
            let events = fixture.events.lock().unwrap();
            let diagnostics: Vec<_> = events
                .iter()
                .filter(|event| event.record.is_err())
                .collect();
            assert_eq!(
                diagnostics.len(),
                1,
                "the worker must report the sibling diagnostic after its outer future is dropped"
            );
            assert_eq!(diagnostics[0].slot, FieldBefore);
            assert_eq!(
                diagnostics[0].path.as_ref(),
                &[chigiri::PathSegment::from("b")]
            );
            if second == 499 {
                assert_eq!(
                    diagnostics[0].record.as_ref().err().unwrap().message,
                    "invalid hook rejection"
                );
            }
        }
        fixture.stop().await;
    }
}

#[tokio::test]
async fn ra_reached_results_merge_independently_of_arrival_and_internal_failure_wins() {
    let _case = crate::vm_gate::test_case();
    for (codes, expected) in [
        ([401, 403], 403),
        ([200, 403], 403),
        ([200, 499], 500),
        ([503, 499], 500),
        ([200, -1], 500),
    ] {
        let query = format!(
            "{{ a:value(requested:{}) b:value(requested:{}) }}",
            codes[0], codes[1]
        );
        for first in [0, 1] {
            let before = [
                Pause::new("a", Stage::BeforeHook),
                Pause::new("b", Stage::BeforeHook),
            ];
            let after = [
                Pause::new("a", Stage::AfterHook),
                Pause::new("b", Stage::AfterHook),
            ];
            let ready = [
                Pause::new("a", Stage::HookResult),
                Pause::new("b", Stage::HookResult),
            ];
            let schedule = Schedule::new(&[
                before[0].clone(),
                before[1].clone(),
                after[0].clone(),
                after[1].clone(),
                ready[0].clone(),
                ready[1].clone(),
            ]);
            let completed = schedule.completed.clone();
            let fixture = Fixture::new(
                "type Query { value(requested:Int!): Int @chigiri_mock_resolver }",
                "{}",
                &[("Query.value", "error('rejected resolver must not run')")],
                &[
                    (FieldBefore, "local code = ctx.field.arguments.requested assert(type(code)=='number') if code == -1 then error('private failure') end ctx.reject({code=code,message='Denied'}) return nil"),
                    (FieldAfter, "ctx.log('info','completed',{result=ctx.result})"),
                ],
            ).scheduled(schedule);
            let aliases = ["a", "b"];
            let (response, ()) = tokio::join!(
                fixture.request(
                    "POST",
                    None,
                    serde_json::to_vec(&json!({"query":query})).unwrap(),
                    &[]
                ),
                async {
                    // Both fields are running but cannot enter the Luau hook until released.
                    before[0].reached().await;
                    before[1].reached().await;
                    assert!(fixture.events.lock().unwrap().is_empty());
                    assert!(completed.lock().unwrap().is_empty());
                    // Each VM retains its session job until its rejected hook result is returned.
                    for index in [0, 1] {
                        before[index].proceed().await;
                        ready[index].reached().await;
                    }
                    for index in [first, 1 - first] {
                        ready[index].proceed().await;
                        after[index].reached().await;
                        after[index].proceed().await;
                        assert_eq!(completed.lock().unwrap().last().unwrap(), aliases[index]);
                    }
                }
            );
            assert_eq!(
                completed.lock().unwrap().as_slice(),
                [aliases[first], aliases[1 - first]]
            );
            assert_eq!(response.0.status, expected);
            let invalid = codes[1] == 499 || codes[1] == -1;
            assert_eq!(response.1, if invalid { None } else { Some(expected) });
            let value: serde_json::Value = serde_json::from_slice(&response.0.body).unwrap();
            assert_eq!(value["data"], json!(null));
            let errors = value["errors"].as_array().unwrap();
            assert_eq!(errors.len(), 1);
            assert_eq!(
                errors[0]["extensions"]["code"],
                if invalid {
                    "MOCK_EXECUTION_FAILED"
                } else {
                    "MOCK_REJECTED"
                }
            );
            assert!(errors[0]["extensions"]["requestId"].is_string());
            if invalid {
                assert!(errors[0]["extensions"].get("httpStatus").is_none());
            } else {
                assert_eq!(errors[0]["extensions"]["httpStatus"], expected);
            }
            assert!(!value.to_string().contains("private failure"));
            {
                let events = fixture.events.lock().unwrap();
                let after: Vec<_> = events
                    .iter()
                    .filter(|event| event.slot == FieldAfter)
                    .collect();
                assert!(after.is_empty());
                let diagnostics: Vec<_> = events
                    .iter()
                    .filter(|event| event.record.is_err())
                    .collect();
                assert_eq!(diagnostics.len(), usize::from(invalid));
                assert!(diagnostics.iter().all(|event| event.slot == FieldBefore
                    && event.path.as_ref()
                        == [chigiri::PathSegment::from(if codes[0] == 499 {
                            "a"
                        } else {
                            "b"
                        })]));
            }
            fixture.stop().await;
        }
    }
}

#[tokio::test]
async fn first_rejection_cancels_unstarted_sibling_hooks_and_preserves_status() {
    let _case = crate::vm_gate::test_case();
    for first in [0, 1] {
        let before = [
            Pause::new("a", Stage::BeforeHook),
            Pause::new("b", Stage::BeforeHook),
        ];
        let after = [
            Pause::new("a", Stage::AfterHook),
            Pause::new("b", Stage::AfterHook),
        ];
        let schedule = Schedule::new(&[
            before[0].clone(),
            before[1].clone(),
            after[0].clone(),
            after[1].clone(),
        ]);
        let fixture = Fixture::new(
            "type Query { value(requested:Int!): Int @chigiri_mock_resolver }", "{}",
            &[("Query.value", "error('resolver must not run')")],
            &[(FieldBefore, "local code = ctx.field.arguments.requested assert(type(code)=='number') ctx.log('info','reached',{code=code}) ctx.reject({code=code}) return nil")],
        ).scheduled(schedule);
        let (response, ()) = tokio::join!(
            fixture.request(
                "POST",
                None,
                br#"{"query":"{a:value(requested:200) b:value(requested:403)}"}"#.to_vec(),
                &[]
            ),
            async {
                before[0].reached().await;
                before[1].reached().await;
                for index in [first, 1 - first] {
                    before[index].proceed().await;
                    after[index].reached().await;
                    after[index].proceed().await;
                }
            }
        );
        let expected = [200, 403][first];
        assert_eq!(response.0.status, expected);
        assert_eq!(response.1, Some(expected));
        let value: serde_json::Value = serde_json::from_slice(&response.0.body).unwrap();
        assert_eq!(value["data"], json!(null));
        assert_eq!(value["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
        let logs = fixture.logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1["code"], expected);
        fixture.stop().await;
    }
}
