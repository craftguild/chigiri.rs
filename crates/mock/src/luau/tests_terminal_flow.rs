//! Verify checker-only terminal refinement without changing executable source.

use super::literal_precision::ColumnMap;
use super::tests::{candidate, contract};

const SCHEMA: &str = "type Query { value(found: Boolean!, timed: Boolean!): String! }";
const SESSION: &str = r#"
type Session = { userId: string, revoked: boolean, expiresAt: number }
local session: Session? = if args.found then { userId = "u", revoked = false, expiresAt = 10 } else nil
"#;
const FAILURE: &str = "{ code = \"UNAUTHENTICATED\", message = \"Authentication required\" }";

#[test]
fn af_08_simple_and_compound_host_fail_guards_refine() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let simple =
        format!("{SESSION}\nif session == nil then ctx.fail({FAILURE}) end\nreturn session.userId");
    contract
        .script(candidate(&contract, "Query.value", &simple))
        .unwrap();

    let compound = format!(
        "{SESSION}\nlocal now: number? = if args.timed then 5 else nil\n\
         if session == nil or session.revoked or now == nil or session.expiresAt <= now then\n\
             ctx.fail({FAILURE})\n\
         end\n\
         return session.userId .. tostring(now)"
    );
    contract
        .script(candidate(&contract, "Query.value", &compound))
        .unwrap();
}

#[test]
fn af_08_typed_helper_alias_and_varargs_refine_without_runtime_rewrite() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let body = format!(
        "{SESSION}\n\
         local function requireSession(helperCtx: ChigiriQueryValueContext, value: Session?, ...): Session\n\
             local stop = helperCtx\n\
             local _first = ...\n\
             if value == nil then stop.fail({FAILURE}) end\n\
             return value\n\
         end\n\
         return requireSession(ctx, session, 'proof').userId"
    );
    let candidate = candidate(&contract, "Query.value", &body);
    let original = candidate.source.clone();
    let script = contract.script(candidate).unwrap();
    assert_eq!(script.source(), original);
    assert!(!script.source().contains("return stop.fail"));
}

#[test]
fn af_08_common_context_helpers_refine_without_type_shadowing() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let body = format!(
        "{SESSION}\n\
         local function requireRead(helperCtx: ChigiriReadContext, value: Session?): Session\n\
             if value == nil then helperCtx.fail({FAILURE}) end\n\
             return value\n\
         end\n\
         local function requireWrite(helperCtx: ChigiriWriteContext, value: Session?): Session\n\
             if value == nil then helperCtx.fail({FAILURE}) end\n\
             return value\n\
         end\n\
         local _write = requireWrite\n\
         return requireRead(ctx, session).userId"
    );
    contract
        .script(candidate(&contract, "Query.value", &body))
        .unwrap();
}

#[test]
fn af_08_reassigned_aliases_never_regain_host_identity() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let flows = [
        "local stop: { fail: (ChigiriBusinessFailure) -> () } = {\n\
             fail = function(_failure: ChigiriBusinessFailure) end,\n\
         }\n\
         if args.timed then stop = ctx end",
        "local stop: { fail: (ChigiriBusinessFailure) -> () } = {\n\
             fail = function(_failure: ChigiriBusinessFailure) end,\n\
         }\n\
         while args.timed do stop = ctx break end",
        "local fake: { fail: (ChigiriBusinessFailure) -> () } = {\n\
             fail = function(_failure: ChigiriBusinessFailure) end,\n\
         }\n\
         local stop: { fail: (ChigiriBusinessFailure) -> () } = ctx\n\
         for _ = 1, 2 do\n\
             stop = fake\n\
             if args.timed then break end\n\
             stop = ctx\n\
         end",
        "local fake: { fail: (ChigiriBusinessFailure) -> () } = {\n\
             fail = function(_failure: ChigiriBusinessFailure) end,\n\
         }\n\
         local stop: { fail: (ChigiriBusinessFailure) -> () } = ctx\n\
         for _ = 1, 2 do\n\
             stop = fake\n\
             if args.timed then continue end\n\
             stop = ctx\n\
         end",
        "local fake: { fail: (ChigiriBusinessFailure) -> () } = {\n\
             fail = function(_failure: ChigiriBusinessFailure) end,\n\
         }\n\
         local stop: { fail: (ChigiriBusinessFailure) -> () } = ctx\n\
         if args.timed then\n\
             stop = fake\n\
             local stop = ctx\n\
             local _inner = stop\n\
         end",
    ];
    for flow in flows {
        let control = format!("{SESSION}\n{flow}\nreturn 'ok'");
        contract
            .script(candidate(&contract, "Query.value", &control))
            .unwrap();
        let body = format!(
            "{SESSION}\n{flow}\n\
             if session == nil then stop.fail({FAILURE}) end\n\
             return session.userId"
        );
        let candidate = candidate(&contract, "Query.value", &body);
        let mut columns = ColumnMap::new(&candidate.source);
        let refined = super::terminal_flow::refine(&candidate.source, &mut columns, contract.ir());
        assert!(!refined.contains("return stop.fail"), "{refined}");
        let error = contract.script(candidate).err().unwrap();
        assert_eq!(error.phase, "type", "{body}");
    }
}

#[test]
fn af_08_following_return_keeps_refined_source_valid() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let body = format!(
        "{SESSION}\nif session == nil then\n\
             ctx.fail({FAILURE})\n\
             return 'fallback'\n\
         end\n\
         return session.userId"
    );
    let candidate = candidate(&contract, "Query.value", &body);
    let original = candidate.source.clone();
    let script = contract.script(candidate).unwrap();
    assert_eq!(script.source(), original);
}

#[test]
fn af_08_shadow_reassignment_protected_call_and_real_nil_stay_invalid() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let cases = [
        format!(
            "{SESSION}\nlocal other = {{ fail = function(_: any) end }}\n\
             if session == nil then other.fail({FAILURE}) end\nreturn session.userId"
        ),
        format!(
            "{SESSION}\nlocal stop: any = ctx\nstop = {{ fail = function(_: any) end }}\n\
             if session == nil then stop.fail({FAILURE}) end\nreturn session.userId"
        ),
        format!(
            "{SESSION}\nif session == nil then\n\
                 pcall(function() ctx.fail({FAILURE}) end)\n\
             end\nreturn session.userId"
        ),
        format!(
            "{SESSION}\ntype ChigiriFakeContext = {{ fail: (any) -> () }}\n\
             local function stop(fake: ChigiriFakeContext)\n\
                 if session == nil then fake.fail({FAILURE}) end\n\
             end\nstop({{fail=function(_: any) end}})\nreturn session.userId"
        ),
        format!(
            "{SESSION}\ntype ChigiriQueryValueContext = {{ fail: (any) -> () }}\n\
             local function stop(fake: ChigiriQueryValueContext)\n\
                 if session == nil then fake.fail({FAILURE}) end\n\
             end\nstop({{fail=function(_: any) end}})\nreturn session.userId"
        ),
        format!(
            "{SESSION}\ntype ChigiriReadContext = {{ fail: (any) -> () }}\n\
             local function stop(fake: ChigiriReadContext)\n\
                 if session == nil then fake.fail({FAILURE}) end\n\
             end\nstop({{fail=function(_: any) end}})\nreturn session.userId"
        ),
        format!(
            "{SESSION}\ntype ChigiriWriteContext = {{ fail: (any) -> () }}\n\
             local function stop(fake: ChigiriWriteContext)\n\
                 if session == nil then fake.fail({FAILURE}) end\n\
             end\nstop({{fail=function(_: any) end}})\nreturn session.userId"
        ),
        format!(
            "{SESSION}\nlocal stop: any = ctx\n\
             local function requireSession(value: Session?): Session\n\
                 if value == nil then stop.fail({FAILURE}) end\n\
                 return value\n\
             end\nstop = {{fail=function(_: any) end}}\nreturn requireSession(session).userId"
        ),
        format!("{SESSION}\nreturn session.userId"),
    ];
    for body in cases {
        let error = contract
            .script(candidate(&contract, "Query.value", &body))
            .err()
            .unwrap();
        assert_eq!(error.phase, "type", "{body}");
    }
}

#[test]
fn af_08_invalid_fail_payload_and_nested_shadow_remain_invalid() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let invalid_payload = format!(
        "{SESSION}\nif session == nil then ctx.fail({{ code = 7 }}) end\nreturn session.userId"
    );
    assert!(
        contract
            .script(candidate(&contract, "Query.value", &invalid_payload))
            .is_err()
    );

    let nested_shadow = format!(
        "{SESSION}\n\
         local function inspect(ctx: {{ fail: (any) -> () }}, value: Session?): string\n\
             if value == nil then ctx.fail({FAILURE}) end\n\
             return value.userId\n\
         end\n\
         return inspect({{ fail = function(_: any) end }}, session)"
    );
    assert!(
        contract
            .script(candidate(&contract, "Query.value", &nested_shadow))
            .is_err()
    );
}

#[test]
fn af_08_unconditional_fail_keeps_its_runtime_continuation_contract() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    let body = format!("ctx.fail({FAILURE})\nreturn 'unreachable'");
    contract
        .script(candidate(&contract, "Query.value", &body))
        .unwrap();
}

#[test]
fn af_08_inserted_return_keeps_type_site_and_both_span_endpoints() {
    let _case = crate::vm_gate::test_case();
    let contract =
        contract("type Item { id: ID! } type Query { items(blocked: Boolean!): [Item!]! }");
    let body = format!(
        "if args.blocked then ctx.fail({FAILURE}) end; return ctx.models.Item:find({{limit='bad'}})"
    );
    let candidate = candidate(&contract, "Query.items", &body);
    let (expected_line, expected_column) = candidate
        .source
        .lines()
        .enumerate()
        .find_map(|(index, line)| line.find("limit").map(|column| (index + 1, column)))
        .unwrap();
    let error = contract.script(candidate).err().unwrap();
    assert_eq!(error.body, (expected_line, expected_column));
    assert_eq!(error.member.as_deref(), Some("Item.find.limit"));
    assert_eq!(error.actual, "string");

    let source = "local function guard(ctx: ChigiriQueryItemsContext)\n    if true then ctx.fail({code='DENIED'}) end\nend\n";
    let mut columns = ColumnMap::new(source);
    let refined = super::terminal_flow::refine(source, &mut columns, contract.ir());
    let original_line = source.lines().nth(1).unwrap();
    let refined_line = refined.lines().nth(1).unwrap();
    for needle in ["ctx.fail", " end"] {
        let original = original_line.find(needle).unwrap() + 1;
        let generated = refined_line.find(needle).unwrap() + 1;
        assert_eq!(columns.original(2, generated), original);
    }
}

#[test]
fn af_08_comments_strings_and_unexecuted_branches_have_ast_meaning() {
    let contract = contract(SCHEMA);
    let source = r#"-- ctx.fail({ code = "comment" })
local text = "ctx.fail({ code = 'string' })"
local function run(ctx: ChigiriQueryValueContext)
    if false then ctx.fail({ code = "real" }) end
end
return text, run
"#;
    let mut columns = ColumnMap::new(source);
    let refined = super::terminal_flow::refine(source, &mut columns, contract.ir());
    assert!(refined.contains("-- ctx.fail({ code = \"comment\" })"));
    assert!(refined.contains("\"ctx.fail({ code = 'string' })\""));
    assert_eq!(
        refined
            .matches("return ((nil :: any) :: () -> never)()")
            .count(),
        1
    );
}
