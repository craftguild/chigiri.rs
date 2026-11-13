//! Compare soft warnings and normal commits over real HTTP with the same request ID.

use std::fs;

use chigiri_mock::hooks::HookSlot;
use chigiri_mock::limits::request::{LimitKind, policy};
use serde_json::{Value, json};

use super::hooks_http::with_hook;
use super::support::{Server, TEMPLATE, edited, seed, with_seed};

#[test]
fn ra_soft_work_warning_preserves_all_hooks_and_same_budget_without_after() {
    let _case = crate::support::test_case();
    let config = edited(
        "Mutation.createUser",
        r#"
for i = 1, 9 do
    local rows = ctx.models.Post:find({limit=20})
    if #rows ~= 20 then error('fixture count') end
    for _, row in rows do
        if #row.body ~= 4096 then error('fixture body') end
    end
end
local id = 'User-' .. (ctx.models.User:count({}) + 1)
local row = ctx.models.User:create({id=id, name=args.input.name})
return row
"#,
    );
    let mut records = seed(TEMPLATE);
    records["Post"] = (1..=20)
        .map(|id| {
            json!({
            "id":format!("Post-{id}"), "title":"", "body":"x".repeat(4096), "authorId":"User-1"
                })
        })
        .collect();
    let config = with_seed(&config, &records);
    let enabled = soft_case(&config, true);
    let disabled = soft_case(&config, false);
    assert_eq!(enabled, disabled);
}

fn soft_case(base: &str, after: bool) -> (Value, Value) {
    let mut config = base.to_owned();
    for slot in HookSlot::ALL {
        if after || slot.timing() == chigiri_mock::hooks::HookTiming::Before {
            config = with_hook(
                &config,
                slot,
                &format!("ctx.log('info', '{}') return nil", slot.name()),
            );
        }
    }
    let mut server = Server::start(&config);
    let http = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{created:createUser(input:{name:\"soft-success\"}){id name}}"})
            .to_string(),
    );
    assert_eq!(http.status, 200, "{}", http.body);
    let response: Value = serde_json::from_str(&http.body).unwrap();
    assert!(response.get("errors").is_none(), "{response}");
    assert_eq!(
        response["data"]["created"],
        json!({"id":"User-3","name":"soft-success"})
    );
    let committed = server.graphql("{getUsers{id name}}");
    assert!(
        committed["data"]["getUsers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row == &json!({"id":"User-3","name":"soft-success"}))
    );
    server.stop("-TERM");
    let stdout = super::logging::records(&server);
    let access = stdout
        .iter()
        .find(|record| record["kind"] == "access")
        .unwrap();
    assert_eq!(access["status"], 200);
    let id = &access["request_id"];
    for slot in HookSlot::ALL {
        let logs: Vec<_> = stdout
            .iter()
            .filter(|record| &record["request_id"] == id && record["message"] == slot.name())
            .collect();
        let expected = if !after && slot.timing() == chigiri_mock::hooks::HookTiming::After {
            0
        } else if matches!(slot, HookSlot::FieldBefore | HookSlot::FieldAfter) {
            3 //Each field of the id/name selected by root.
        } else {
            1
        };
        assert_eq!(logs.len(), expected, "after={after}, {}", slot.name());
        for log in logs {
            assert_eq!(log["phase"], serde_json::to_value(slot.phase()).unwrap());
            assert_eq!(log["timing"], serde_json::to_value(slot.timing()).unwrap());
        }
    }
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let warnings: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == "limit_warning" && &record["request_id"] == id)
        .collect();
    let work: Vec<_> = warnings
        .iter()
        .filter(|record| record["limit"] == "work")
        .collect();
    assert_eq!(work.len(), 1, "{stderr}");
    let policy = policy(LimitKind::Work);
    assert_eq!((policy.soft, policy.hard), (750_000, 1_000_000));
    assert_eq!(work[0]["level"], "warn");
    assert_eq!(work[0]["phase"], "resolver");
    assert_eq!(work[0]["coordinate"], "Mutation.createUser");
    assert_eq!(work[0]["path"], json!(["created"]));
    assert_eq!(work[0]["soft"], policy.soft);
    assert_eq!(work[0]["hard"], policy.hard);
    assert!(
        (policy.soft..=policy.hard).contains(&(work[0]["observed"].as_u64().unwrap() as usize))
    );
    let terminals: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == "request_terminal" && &record["request_id"] == id)
        .collect();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0]["transaction"], "committed");
    assert_eq!(terminals[0]["cause"], "success");
    assert_eq!(terminals[0]["unreached_after_hooks"], json!([]));
    (response, committed)
}
