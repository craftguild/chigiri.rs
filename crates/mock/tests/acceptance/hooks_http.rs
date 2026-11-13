//! Send a test bundle to the real binary and verify HTTP rejection status and transport priority.

use chigiri_mock::hooks::HookSlot;
use chigiri_mock::luau::render_hook_module;
use serde_json::{Value, json};

use super::support::{Server, TEMPLATE, seed, with_seed};

pub(super) fn with_hook(config: &str, slot: HookSlot, body: &str) -> String {
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    let (phase, timing) = slot.name().split_once('.').unwrap();
    document["hooks"][phase][timing] = toml_edit::value(true);
    let mut module = toml_edit::Table::new();
    module["luau"] = toml_edit::value(render_hook_module(slot, body).unwrap());
    document["hooks"]["modules"][slot.name()] = toml_edit::Item::Table(module);
    document.to_string()
}

fn nullable_roots(config: &str) -> String {
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    let sources = document["schema"]["sources"]
        .as_array_of_tables_mut()
        .unwrap();
    let mut inputs = Vec::new();
    for source in sources.iter_mut() {
        if source["name"].as_str().unwrap() == chigiri_mock::schema::VOCABULARY_SOURCE {
            continue;
        }
        let text = source["text"].as_str().unwrap().replace(
            "getUsers(limit: Int! = 20): [User!]!",
            "getUsers(limit: Int! = 20): [User!]",
        );
        inputs.push(chigiri::core::Source::new(
            source["name"].as_str().unwrap(),
            text.clone(),
        ));
        source["text"] = toml_edit::value(text);
    }
    let ir = chigiri_mock::ir::MockIr::parse_with_storage(inputs, Vec::new()).unwrap();
    document["schema"]["digest"] = toml_edit::value(ir.public_graph().source().digest().to_hex());
    document["storage"]["digest"] = toml_edit::value(ir.graph().source().digest().to_hex());
    document.to_string()
}

fn status_config() -> String {
    let mut config = TEMPLATE.to_owned();
    for slot in [
        HookSlot::RequestBefore,
        HookSlot::OperationBefore,
        HookSlot::FieldBefore,
    ] {
        let phase = slot.name().split_once('.').unwrap().0;
        config = with_hook(
            &config,
            slot,
            &format!(
                r#"
            local stage, code = '', ''
            for _, header in ctx.request.headers do
                if header.name == 'x-stage' then stage = header.value end
                if header.name == 'x-code' then code = header.value end
            end
            if stage == '{phase}' then
                local codes = {{['200']=200,['401']=401,['403']=403,['600']=600,['499']=499,['100']=100,['401.5']=401.5,['204']=204,['205']=205,['304']=304}}
                local reason: any = {{code=codes[code],message='Denied'}}
                if code == 'text' then reason.code = '401' end
                if code == 'nan' then reason.code = 0/0 end
                if code == 'inf' then reason.code = 1/0 end
                pcall(function() ctx.reject(reason) end)
                pcall(function() ctx.reject({{code=511}}) end)
            end
            return nil
        "#
            ),
        );
    }
    config
}

#[test]
fn hk_31_real_http_all_before_status_invalid_normalization_and_bodyless_codes() {
    let _case = crate::support::test_case();
    let mut server = Server::start(&status_config());
    let document = json!({"query":"{ denied:getUsers { id } }"}).to_string();
    let mut statuses = Vec::new();
    for phase in ["request", "operation", "field"] {
        for (input, expected) in [
            ("200", 200),
            ("401", 401),
            ("403", 403),
            ("600", 500),
            ("499", 500),
            ("100", 500),
            ("401.5", 500),
            ("nan", 500),
            ("inf", 500),
            ("text", 500),
            ("204", 500),
            ("205", 500),
            ("304", 500),
        ] {
            let response = server.http(
                "POST",
                &[
                    ("Content-Type", "application/json"),
                    ("X-Stage", phase),
                    ("X-Code", input),
                ],
                &document,
            );
            assert_eq!(
                response.status, expected,
                "{phase}/{input}: {}",
                response.body
            );
            statuses.push(expected);
            let body: Value = serde_json::from_str(&response.body).unwrap();
            assert!(body["data"].is_null());
            assert_eq!(body["errors"].as_array().unwrap().len(), 1);
            let error = &body["errors"][0];
            let request_id = error["extensions"]["requestId"].as_str().unwrap();
            assert!(request_id.parse::<u64>().unwrap() > 0);
            if expected == 500 {
                assert_eq!(
                    error["extensions"],
                    json!({
                        "code":"MOCK_EXECUTION_FAILED", "requestId":request_id
                    })
                );
                assert_eq!(error["message"], "Mock execution failed");
            } else {
                assert_eq!(
                    error["extensions"],
                    json!({
                        "code":"MOCK_REJECTED", "httpStatus":expected, "requestId":request_id
                    })
                );
                assert_eq!(error["message"], "Denied");
            }
        }
    }
    assert!(server.graphql("{getUsers{id}}")["errors"].is_null());
    statuses.push(200);
    server.stop("-TERM");
    super::logging::assert_access_statuses(&server, &statuses);
}

#[test]
fn hk_32_real_http_rejection_stays_within_entered_hooks_and_skips_after() {
    let _case = crate::support::test_case();
    let mut records = seed(TEMPLATE);
    records["Post"]=(1..=20).map(|id|json!({"id":format!("Post-{id}"),"title":"","body":"x".repeat(4096),"authorId":"User-1"})).collect();
    let config = with_hook(
        &with_seed(TEMPLATE, &records),
        HookSlot::FieldBefore,
        r#"
        if ctx.field.path[1] == 'low' then ctx.log('info','entered low') ctx.reject({code=200}) end
        if ctx.field.path[1] == 'high' then ctx.log('info','entered high') ctx.reject({code=403}) end
        if ctx.field.path[1] == 'invalid' then ctx.log('info','entered invalid') ctx.reject({code=600}) end
        return nil
    "#,
    );
    let ordered = with_hook(
        &config,
        HookSlot::FieldAfter,
        "if #ctx.field.path == 1 and ctx.field.path[1] ~= 'getUsers' then ctx.log('info','rejection completed') end return nil",
    );
    let mut server = Server::start(&nullable_roots(&ordered));
    let mut outcomes = Vec::new();
    for query in [
        "{low:getUsers{id} high:getUsers{id}}",
        "{high:getUsers{id} low:getUsers{id}}",
        "{invalid:getUsers{id} high:getUsers{id}}",
        "{high:getUsers{id} invalid:getUsers{id}}",
    ] {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &json!({"query":query}).to_string(),
        );
        let body: Value = serde_json::from_str(&response.body).unwrap();
        assert!(body["data"].is_null());
        assert_eq!(body["errors"].as_array().unwrap().len(), 1);
        outcomes.push((response.status, body));
    }
    assert!(server.graphql("{getUsers{id}}")["errors"].is_null());
    server.stop("-TERM");
    let logs = super::logging::records(&server);
    for (status, body) in &outcomes {
        let request = body["errors"][0]["extensions"]["requestId"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let events: Vec<_> = logs
            .iter()
            .filter(|log| log["request_id"] == request)
            .collect();
        let entered: Vec<_> = events
            .iter()
            .filter_map(|log| match log["message"].as_str() {
                Some("entered low") => Some(200),
                Some("entered high") => Some(403),
                Some("entered invalid") => Some(500),
                _ => None,
            })
            .collect();
        // Remove the log before shutdown so recreation can be observed directly.
        // A controlled service test verifies the strict rejection limit separately.
        assert!(entered.contains(status), "{status}: {entered:?}");
        let code = if *status == 500 {
            "MOCK_EXECUTION_FAILED"
        } else {
            "MOCK_REJECTED"
        };
        assert_eq!(body["errors"][0]["extensions"]["code"], code);
        assert!(
            !events
                .iter()
                .any(|log| log["message"] == "rejection completed")
        );
    }
    let mut statuses: Vec<_> = outcomes.iter().map(|(status, _)| *status).collect();
    statuses.push(200);
    super::logging::assert_access_statuses(&server, &statuses);
    // After rejection, sibling results cannot turn the response into success.
    let mut server = Server::start(&nullable_roots(&config));
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"{low:getUsers{id} getUsers(limit:1){posts{author{posts{body}}}}}"})
            .to_string(),
    );
    assert_eq!(response.status, 200, "{}", response.body);
    assert!(response.body.len() < 1024);
    let body: Value = serde_json::from_str(&response.body).unwrap();
    assert!(body["data"].is_null());
    assert_eq!(body["errors"].as_array().unwrap().len(), 1);
    assert_eq!(body["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
    assert!(server.graphql("{getUsers{id}}")["errors"].is_null());
    server.stop("-TERM");
    super::logging::assert_access_statuses(&server, &[200, 200]);
}

#[test]
fn hk_18_authorization_preflight_keeps_exact_origin_and_closed_header_allowlist() {
    let _case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    for (origin, headers, status) in [
        (
            "http://localhost:5173",
            "Authorization, content-TYPE, ACCEPT",
            204,
        ),
        ("http://localhost:5173", "Authorization, x-unapproved", 403),
        ("http://localhost:5173.evil", "Authorization", 403),
        ("null", "Authorization", 403),
    ] {
        let response = server.http(
            "OPTIONS",
            &[
                ("Origin", origin),
                ("Access-Control-Request-Method", "POST"),
                ("Access-Control-Request-Headers", headers),
            ],
            "",
        );
        assert_eq!(response.status, status);
        assert!(!response.headers.contains("allow-credentials"));
        if status == 204 {
            assert!(
                response
                    .headers
                    .contains("access-control-allow-headers: content-type, accept, authorization")
            );
            assert!(
                response
                    .headers
                    .contains("access-control-allow-origin: http://localhost:5173")
            );
        }
    }
    server.stop("-TERM");
}

#[test]
fn parallel_field_rejections_are_order_independent_and_not_internal_failures() {
    let _case = crate::support::test_case();
    let config = with_hook(
        &nullable_roots(TEMPLATE),
        HookSlot::FieldBefore,
        "ctx.reject({code=403,message='Denied'}) return nil",
    );
    let mut server = Server::start(&config);
    for query in [
        "{a:getUsers{id} b:getUsers{id}}",
        "{b:getUsers{id} a:getUsers{id}}",
    ] {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &json!({"query":query}).to_string(),
        );
        if response.status != 403 {
            server.stop("-TERM");
        }
        assert_eq!(
            response.status,
            403,
            "{}; {}",
            response.body,
            std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap()
        );
        let body: Value = serde_json::from_str(&response.body).unwrap();
        assert!(body["data"].is_null());
        assert_eq!(body["errors"].as_array().unwrap().len(), 1);
        assert_eq!(body["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
    }
    server.stop("-TERM");
}
