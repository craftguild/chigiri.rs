//! Verify all six hook slots enforce HTTP resource failures, rollback, and same-server recovery.

use std::time::{Duration, Instant};

use chigiri_mock::hooks::HookSlot;
use serde_json::{Value, json};

use super::hooks_http::with_hook;
use super::support::{Server, edited};

const MODES: [&str; 9] = [
    "loop",
    "caught-loop",
    "memory",
    "caught-memory",
    "pcall17",
    "pcall16",
    "caught-error",
    "host256",
    "host257",
];

const ATTACK: &str = r#"
ctx.log('info', 'resource-entered', {mode=mode})
local function allocate(): ()
    local value = 'x'
    for _ = 1, 30 do value = value .. value end
end
local function descend(n: number): ()
    if n > 0 then pcall(descend, n - 1) end
end
if mode == 'loop' then
    while true do end
elseif mode == 'caught-loop' then
    pcall(function() while true do end end)
elseif mode == 'memory' then
    allocate()
elseif mode == 'caught-memory' then
    pcall(allocate)
elseif mode == 'pcall17' then
    descend(17)
elseif mode == 'pcall16' then
    for i = 1, 4 do
        descend(16)
    end
elseif mode == 'host256' then
    for _ = 1, 254 do ctx.models.User:count({}) end
elseif mode == 'host257' then
    for _ = 1, 256 do ctx.models.User:count({}) end
elseif mode == 'caught-error' then
    local ok = pcall(function() error('ordinary') end)
    assert(not ok)
end
ctx.log('info', 'resource-returned', {mode=mode})
"#;

fn configuration(slot: HookSlot) -> String {
    // Keep complex HTTP fixtures separate from smaller library tests without
    // changing VM stack, time, or memory limits.
    std::thread::Builder::new()
        .name("hook-resource-fixture".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || configuration_source(slot))
        .unwrap()
        .join()
        .unwrap()
}

fn configuration_source(slot: HookSlot) -> String {
    let mut config = edited(
        "Mutation.createPost",
        r#"
if args.input.title == 'resolver-fail' then
    ctx.models.User:create({id='discarded', name='discarded'})
end
local post = ctx.models.Post:create(args.input)
if args.input.title == 'resolver-fail' then while true do end end
return post
"#,
    );
    let read_mode = r#"
local mode = ''
for _, header in ctx.request.headers do
    if header.name == 'x-resource-mode' then mode = header.value end
end
"#;
    let patch = "return {statePatch={mode=mode}}";
    if slot != HookSlot::RequestBefore {
        config = with_hook(
            &config,
            HookSlot::RequestBefore,
            &format!("{read_mode}\n{patch}"),
        );
    }
    let mut body = if slot == HookSlot::RequestBefore {
        read_mode.to_owned()
    } else {
        "local mode = ctx.state.mode\n".into()
    };
    if matches!(slot, HookSlot::FieldBefore | HookSlot::FieldAfter) {
        body.push_str("if #ctx.field.path ~= 1 or ctx.field.coordinate ~= 'Mutation.createPost' then return nil end\n");
    }
    body.push_str(&format!("if mode ~= '' then\n{ATTACK}\nend\n"));
    body.push_str(if slot == HookSlot::RequestBefore {
        patch
    } else {
        "return nil"
    });
    config = with_hook(&config, slot, &body);
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    document["state"]["contract_json"] = toml_edit::value(
        json!({"fields":{"mode":{"kind":"string"}},"initial":{"mode":""}}).to_string(),
    );
    document.to_string()
}

fn mutation(server: &Server, mode: &str, title: &str) -> Value {
    let query = format!(
        "mutation{{first:createUser(input:{{name:\"retained\"}}){{id}} second:createPost(input:{{title:\"{title}\",body:\"body\",authorId:\"User-1\"}}){{id author{{id}}}}}}"
    );
    let response = server.http(
        "POST",
        &[
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
            ("X-Resource-Mode", mode),
        ],
        &json!({"query":query}).to_string(),
    );
    let expected =
        if (mode.is_empty() || matches!(mode, "pcall16" | "host256")) && title != "resolver-fail" {
            200
        } else {
            500
        };
    assert_eq!(
        response.status, expected,
        "{mode}/{title}: {}",
        response.body
    );
    serde_json::from_str(&response.body).unwrap()
}

fn success(result: &Value, users: &mut usize, posts: &mut usize) {
    assert_eq!(
        result,
        &json!({"data":{
            "first":{"id":format!("User-{}", *users + 1)},
            "second":{"id":format!("Post-{}", *posts + 1),"author":{"id":"User-1"}}
        }})
    );
    *users += 1;
    *posts += 1;
}

fn store(server: &Server, users: usize, posts: usize) {
    let result =
        server.graphql("{getUsers(limit:100){id posts{id}} findPost(limit:100){id author{id}}}");
    assert!(result["errors"].is_null(), "{result}");
    let user_rows = result["data"]["getUsers"].as_array().unwrap();
    let post_rows = result["data"]["findPost"].as_array().unwrap();
    assert_eq!(user_rows.len(), users);
    assert_eq!(post_rows.len(), posts);
    for (index, row) in user_rows.iter().enumerate() {
        assert_eq!(row["id"], format!("User-{}", index + 1));
        let related = if index == 0 {
            (1..=posts)
                .map(|id| json!({"id":format!("Post-{id}")}))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        assert_eq!(row["posts"], json!(related));
    }
    for (index, row) in post_rows.iter().enumerate() {
        assert_eq!(
            row,
            &json!({"id":format!("Post-{}",index + 1),"author":{"id":"User-1"}})
        );
    }
}

fn logs(server: &Server, slot: HookSlot) {
    let records = super::logging::records(server);
    for mode in MODES {
        let entered = records
            .iter()
            .filter(|record| {
                record["message"] == "resource-entered" && record["fields"]["mode"] == mode
            })
            .collect::<Vec<_>>();
        assert_eq!(entered.len(), 1, "{}/{mode}", slot.name());
        let id = &entered[0]["request_id"];
        assert_eq!(
            entered[0]["phase"],
            serde_json::to_value(slot.phase()).unwrap()
        );
        assert_eq!(
            entered[0]["timing"],
            serde_json::to_value(slot.timing()).unwrap()
        );
        let same_request = records
            .iter()
            .filter(|record| &record["request_id"] == id)
            .collect::<Vec<_>>();
        let stderr = std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
        let diagnostics: Vec<serde_json::Value> = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect();
        let failures = diagnostics
            .iter()
            .filter(|record| &record["request_id"] == id)
            .filter(|record| record["diagnostic"].is_object())
            .count();
        if let Some((name, hard, observed)) = local_limit(mode) {
            let terminal: Vec<_> = diagnostics
                .iter()
                .filter(|record| {
                    &record["request_id"] == id && record["kind"] == "request_terminal"
                })
                .collect();
            assert_eq!(terminal.len(), 1);
            assert_eq!(terminal[0]["cause"], "hard_limit");
            assert_eq!(terminal[0]["transaction"], "rolled_back");
            assert_eq!(
                terminal[0]["limit"],
                json!({
                    "name":name, "soft":null, "hard":hard, "observed":observed
                })
            );
        }
        let returned = same_request
            .iter()
            .filter(|record| record["message"] == "resource-returned")
            .count();
        assert_eq!(
            failures,
            usize::from(!matches!(mode, "pcall16" | "host256")),
            "{}/{mode}",
            slot.name()
        );
        assert_eq!(
            returned,
            usize::from(matches!(mode, "pcall16" | "host256")),
            "{}/{mode}",
            slot.name()
        );
        let access = same_request
            .iter()
            .filter(|record| record["kind"] == "access")
            .collect::<Vec<_>>();
        assert_eq!(access.len(), 1);
        assert_eq!(
            access[0]["status"],
            if matches!(mode, "pcall16" | "host256") {
                200
            } else {
                500
            }
        );
    }
    assert!(records.iter().all(|record| record["kind"] != "log_loss"));
}

fn wait_for_terminals(server: &Server, expected: usize) {
    // This is not the stderr-saturation test: submit four requests serially instead of filling the queue in bursts.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let stderr = std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
        let count = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|record| record["kind"] == "request_terminal")
            .count();
        if count >= expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal delivery incomplete: {count}/{expected}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn hk_12_13_all_six_slots_stop_resources_roll_back_and_recover_same_http_server() {
    let _case = crate::support::test_case();
    for slot in HookSlot::ALL {
        let mut server = Server::start(&configuration(slot));
        let (mut users, mut posts) = (2, 1);
        for (index, mode) in MODES.into_iter().enumerate() {
            let started = Instant::now();
            let result = mutation(&server, mode, mode);
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{}/{mode}",
                slot.name()
            );
            if matches!(mode, "pcall16" | "host256") {
                success(&result, &mut users, &mut posts);
            } else {
                assert!(result["data"].is_null());
                assert_eq!(result["errors"].as_array().unwrap().len(), 1);
                let code = if mode.contains("loop") {
                    "MOCK_EXECUTION_TIMEOUT"
                } else if local_limit(mode).is_some() {
                    "MOCK_EXECUTION_LIMIT_EXCEEDED"
                } else {
                    "MOCK_EXECUTION_FAILED"
                };
                assert_eq!(result["errors"][0]["extensions"]["code"], code);
                if let Some((name, _, _)) = local_limit(mode) {
                    assert_eq!(result["errors"][0]["extensions"]["limit"], name);
                }
            }
            store(&server, users, posts);
            success(&mutation(&server, "", "recovery"), &mut users, &mut posts);
            store(&server, users, posts);
            wait_for_terminals(&server, (index + 1) * 4);
            println!(
                "{} / {mode}: resource bounded, commits/models/order/counters/relations and next request verified",
                slot.name()
            );
        }
        // With the hook enabled, roll back both models and the previous root's changes.
        let failed = mutation(&server, "", "resolver-fail");
        assert_eq!(
            failed["errors"][0]["extensions"]["code"],
            "MOCK_EXECUTION_TIMEOUT"
        );
        assert!(failed["data"].is_null());
        store(&server, users, posts);
        success(
            &mutation(&server, "", "after-rollback"),
            &mut users,
            &mut posts,
        );
        store(&server, users, posts);
        server.stop("-TERM");
        logs(&server, slot);
    }
}

fn local_limit(mode: &str) -> Option<(&'static str, usize, Option<usize>)> {
    match mode {
        "memory" | "caught-memory" => Some(("vmMemoryBytes", 16 * 1024 * 1024, None)),
        "pcall17" => Some(("pcallReentry", 16, Some(17))),
        "host257" => Some(("hostCalls", 256, Some(257))),
        _ => None,
    }
}
