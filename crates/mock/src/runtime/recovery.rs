//! Convert panics at existing VM execution boundaries into independent recoverable failures.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::ExecutionBudget;
use crate::luau::ScriptError;

pub(super) fn run<T>(
    budget: &ExecutionBudget,
    failure: impl FnOnce() -> ScriptError,
    operation: impl FnOnce() -> Result<T, ScriptError>,
) -> Result<T, ScriptError> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        budget.0.clone().fail_independent();
        Err(failure())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::request::{BudgetFailure, LimitKind, LimitNotice};

    #[test]
    fn outer_panic_has_priority_over_hard_failure_and_preserves_deadline() {
        for original in [
            None,
            Some(BudgetFailure::Cancelled),
            Some(BudgetFailure::Deadline),
            Some(BudgetFailure::Hard(LimitNotice {
                kind: LimitKind::Work,
                observed: 1_000_001,
            })),
        ] {
            let budget = ExecutionBudget::default();
            if let Some(original) = original {
                budget.0.clone().fail_with(original);
            }
            let result: Result<(), ScriptError> = run(
                &budget,
                || ScriptError::new("panic", "VM execution failed"),
                || panic!("controlled outer boundary panic"),
            );
            let failure = result.unwrap_err();
            assert_eq!(failure.phase, "panic");
            assert!(!failure.cancelled);
            assert!(failure.stack.is_none());
            let expected = match original {
                None | Some(BudgetFailure::Cancelled) => Some(BudgetFailure::Execution),
                _ => original,
            };
            assert_eq!(budget.failure(), expected);
        }
    }
}
