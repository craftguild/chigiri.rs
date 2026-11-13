//! Serialize closed subscription lifecycle decisions without retaining application data.

use chigiri::__private::SubscriptionTermination;
use chigiri::SubscriptionOverflow;
use serde::Serialize;

use super::records::{Envelope, json};
use crate::publication::MAX_SAFE_SEQUENCE;
use crate::value::ValueError;

#[derive(Clone, Copy)]
pub(super) enum SetupOutcome {
    Registered {
        subscription_id: u64,
        overflow: SubscriptionOverflow,
    },
    Failed,
    Cancelled,
}

#[derive(Serialize)]
struct Setup<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    subscription_id: Option<u64>,
    coordinate: &'a str,
    overflow: Option<&'static str>,
    outcome: &'static str,
}

#[derive(Serialize)]
struct DropRecord<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    subscription_id: u64,
    coordinate: &'a str,
    reason: &'static str,
    dropped_total: u64,
}

#[derive(Serialize)]
struct Terminal<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    subscription_id: u64,
    coordinate: &'a str,
    outcome: &'static str,
    cause: &'static str,
    dropped_total: u64,
}

pub(super) fn setup(
    request_id: u64,
    coordinate: &str,
    decision: SetupOutcome,
) -> Result<Vec<u8>, ValueError> {
    let (subscription_id, overflow, outcome) = match decision {
        SetupOutcome::Registered {
            subscription_id,
            overflow,
        } => {
            generation(subscription_id)?;
            (
                Some(subscription_id),
                Some(overflow_name(overflow)),
                "registered",
            )
        }
        SetupOutcome::Failed => (None, None, "failed"),
        SetupOutcome::Cancelled => (None, None, "cancelled"),
    };
    json(&Setup {
        envelope: Envelope::new(Some(request_id)),
        kind: "subscription_setup",
        subscription_id,
        coordinate,
        overflow,
        outcome,
    })
}

pub(super) fn drop_oldest(
    subscription_id: u64,
    coordinate: &str,
    dropped_total: u64,
) -> Result<Vec<u8>, ValueError> {
    generation(subscription_id)?;
    if dropped_total == 0 || dropped_total > MAX_SAFE_SEQUENCE {
        return Err(ValueError("invalid subscription drop total"));
    }
    json(&DropRecord {
        envelope: Envelope::new(None),
        kind: "subscription_drop",
        subscription_id,
        coordinate,
        reason: "drop_oldest",
        dropped_total,
    })
}

pub(super) fn terminal(
    subscription_id: u64,
    coordinate: &str,
    decision: SubscriptionTermination,
    dropped_total: u64,
) -> Result<Vec<u8>, ValueError> {
    generation(subscription_id)?;
    if dropped_total > MAX_SAFE_SEQUENCE {
        return Err(ValueError("invalid subscription terminal drop total"));
    }
    let (outcome, cause) = decision.log_pair();
    json(&Terminal {
        envelope: Envelope::new(None),
        kind: "subscription_terminal",
        subscription_id,
        coordinate,
        outcome,
        cause,
        dropped_total,
    })
}

const fn overflow_name(overflow: SubscriptionOverflow) -> &'static str {
    match overflow {
        SubscriptionOverflow::Backpressure => "backpressure",
        SubscriptionOverflow::DropOldest => "drop_oldest",
    }
}

fn generation(subscription_id: u64) -> Result<(), ValueError> {
    if subscription_id == 0 || subscription_id > MAX_SAFE_SEQUENCE {
        Err(ValueError("invalid subscription generation"))
    } else {
        Ok(())
    }
}
