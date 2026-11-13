//! A managed callback and token destruction have separate unwind boundaries.

use std::panic::{AssertUnwindSafe, catch_unwind};

use chigiri_http::{ResponseCompletionOutcome, ResponseParts};

use crate::managed::EventActivity;
use crate::subscription::driver::SubscriptionCompletion;
use crate::subscription::worker::{dispose_payload, dispose_value};

pub(super) fn finish(
    completion: SubscriptionCompletion,
    response: ResponseParts,
    outcome: ResponseCompletionOutcome,
    activity: &EventActivity,
) -> Result<ResponseParts, ()> {
    let token = completion.into_token();
    let result = match &token {
        Some(token) => catch_unwind(AssertUnwindSafe(|| token.complete(response, outcome))),
        None => Ok(response),
    };
    let disposed = token.is_none_or(dispose_value);
    if !disposed {
        activity.poison_task();
    }
    match result {
        Ok(response) if disposed => Ok(response),
        Ok(response) => {
            if !dispose_value(response) {
                activity.poison_task();
            }
            Err(())
        }
        Err(payload) => {
            activity.fail_task();
            if !dispose_payload(payload) {
                activity.poison_task();
            }
            Err(())
        }
    }
}
