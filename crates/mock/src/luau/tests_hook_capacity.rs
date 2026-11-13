//! Reject aggregate resolver and disabled-hook input above 16 MiB before type checking.

use super::{MAX_GENERATED, ScriptCandidate, static_candidates, static_hooks};

#[test]
fn hk_05_hook_wrappers_share_remaining_capacity_with_resolvers_before_typechecking() {
    let _case = crate::vm_gate::test_case();
    let fields = (0..300)
        .map(|id| format!("f{id}{}: String!\n", "x".repeat(180)))
        .collect::<String>();
    let roots = (0..64)
        .map(|id| format!("r{id}: String!\n"))
        .collect::<String>();
    let contract = super::tests::contract(&format!(
        "type Item {{id:ID! {fields}}} type Query {{{roots}}}"
    ));
    let hooks = static_hooks().unwrap();
    let sources = || hooks.iter().map(|hook| (hook.slot, hook.source.as_str()));
    let hook_bytes = contract.hook_capacity(sources(), MAX_GENERATED).unwrap();
    let mut selected = Vec::<ScriptCandidate>::new();
    let mut used = 0;
    for mut candidate in static_candidates(contract.ir()).unwrap() {
        let size = contract
            .check_capacity(std::slice::from_ref(&candidate))
            .unwrap();
        if used + size > MAX_GENERATED {
            break;
        }
        // An oversized invalid hook must fail capacity checks before resolver syntax validation.
        let length = candidate.source.len();
        candidate.source = "return (".into();
        candidate
            .source
            .push_str(&" ".repeat(length - candidate.source.len()));
        selected.push(candidate);
        used += size;
    }
    assert!(!selected.is_empty());
    assert!(used <= MAX_GENERATED);
    assert!(hook_bytes <= MAX_GENERATED);
    assert!(used + hook_bytes > MAX_GENERATED);
    assert_eq!(contract.check_capacity(&selected).unwrap(), used);
    assert_eq!(
        contract
            .hook_capacity(sources(), MAX_GENERATED - used)
            .unwrap_err()
            .phase,
        "limit"
    );
    assert_eq!(
        contract.hook_capacity(sources(), hook_bytes).unwrap(),
        hook_bytes
    );
    assert_eq!(
        contract
            .hook_capacity(sources(), hook_bytes - 1)
            .unwrap_err()
            .phase,
        "limit"
    );
}
