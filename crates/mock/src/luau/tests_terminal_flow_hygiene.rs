//! Keep checker-only terminal refinement independent of user-local type aliases.

use super::tests::{candidate, contract};
use super::{HookCandidate, render_hook_module};
use crate::hooks::HookSlot;

const SCHEMA: &str = "type Query { value(found: Boolean!): String! }";
const VALID_FAILURE: &str = "{ code = 'DENIED', message = 'Access denied' }";
const INVALID_FAILURE: &str = "{ code = 7, message = 42 }";

#[test]
fn r51_01_resolver_failure_alias_cannot_change_payload_checking() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    for (alias, payload, explicit, accepted) in [
        ("any", INVALID_FAILURE, false, false),
        ("any", INVALID_FAILURE, true, false),
        ("number", VALID_FAILURE, false, true),
        ("number", VALID_FAILURE, true, true),
    ] {
        let call = if explicit {
            format!("return helperCtx.fail({payload})")
        } else {
            format!("helperCtx.fail({payload})")
        };
        let body = format!(
            "type Session = {{ userId: string }}\n\
             local session: Session? = if args.found then {{userId='u'}} else nil\n\
             type ChigiriBusinessFailure = {alias}\n\
             local function requireSession(\n\
                 helperCtx: ChigiriQueryValueContext, value: Session?\n\
             ): Session\n\
                 if value == nil then {call} end\n\
                 return value\n\
             end\n\
             return requireSession(ctx, session).userId"
        );
        let candidate = candidate(&contract, "Query.value", &body);
        let original = candidate.source.clone();
        let result = contract.script(candidate);
        assert_eq!(result.is_ok(), accepted, "{body}");
        if let Ok(script) = result {
            assert_eq!(script.source(), original);
        }
    }
}

#[test]
fn r51_01_hook_helper_failure_alias_cannot_change_payload_checking() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SCHEMA);
    for (alias, payload, accepted) in [
        ("any", INVALID_FAILURE, false),
        ("number", VALID_FAILURE, true),
    ] {
        let body = format!(
            "type ChigiriBusinessFailure = {alias}\n\
             local value: string? = nil\n\
             local function requireValue(\n\
                 helperCtx: ChigiriRequestBeforeContext, item: string?\n\
             ): string\n\
                 if item == nil then helperCtx.fail({payload}) end\n\
                 return item\n\
             end\n\
             local _value = requireValue(ctx, value)\n\
             return nil"
        );
        let source = render_hook_module(HookSlot::RequestBefore, &body).unwrap();
        let result = contract.hooks(
            vec![HookCandidate {
                slot: HookSlot::RequestBefore,
                source: source.clone(),
            }],
            None,
        );
        assert_eq!(result.is_ok(), accepted, "{body}");
        if let Ok(scripts) = result {
            assert_eq!(scripts[0].source(), source);
        }
    }
}
