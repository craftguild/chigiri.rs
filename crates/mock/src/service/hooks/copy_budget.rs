//! Prevent borrowed value handles from refunding aggregate budget between concurrent observation copies.

use crate::limits::warning::LimitOrigin;
use crate::value::{ValueBudget, work::Work};
use std::sync::Mutex;

pub(super) fn at<T>(
    budget: &Mutex<ValueBudget>,
    origin: LimitOrigin,
    copy: impl FnOnce(&mut ValueBudget) -> T,
) -> T {
    let mut budget = budget.lock().expect("observation budget lock");
    let scoped = budget.work.at(origin);
    let previous = std::mem::replace(&mut budget.work, scoped);
    let scope = Scope {
        budget: &mut budget,
        previous,
    };
    copy(scope.budget)
}

struct Scope<'a> {
    budget: &'a mut ValueBudget,
    previous: Work,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        // Restore the previous origin even when copying returns an error.
        self.budget.work = self.previous.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::request::{LimitKind, policy};
    use std::sync::Arc;

    #[test]
    fn copy_failure_restores_the_previous_origin_and_preserves_budget_failure() {
        let work = Work::default();
        work.clone()
            .charge(policy(LimitKind::Work).soft - 1)
            .unwrap();
        let notices = Arc::new(Mutex::new(Vec::new()));
        let received = notices.clone();
        work.with_warnings(move |notice| received.lock().unwrap().push(notice));
        let budget = Mutex::new(ValueBudget::with_work(work.clone()));
        let result: Result<(), ()> = at(
            &budget,
            LimitOrigin {
                phase: "field.after",
                ..Default::default()
            },
            |budget| {
                budget.container(1).unwrap();
                Err(())
            },
        );
        assert!(result.is_err());
        assert_eq!(budget.lock().unwrap().nodes(), 1);
        at(
            &budget,
            LimitOrigin {
                phase: "request.after",
                ..Default::default()
            },
            |budget| {
                budget.container(1).unwrap();
                assert!(budget.work.charge(usize::MAX).is_err());
            },
        );
        assert_eq!(budget.lock().unwrap().nodes(), 2);
        assert!(work.failure().is_some());
        let notices = notices.lock().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].origin.phase, "field.after");
    }
}
