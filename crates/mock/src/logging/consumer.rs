//! Bound explicit consumer logs and emit payload-free transaction outcomes.

use serde::Serialize;

use super::records::{Envelope, json};
use super::{Kind, Logger};
use crate::runtime::HookLog;
use crate::value::serialize::BorrowedObject;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConsumerOutcome {
    Committed,
    RolledBack,
    Stopped,
}

#[derive(Serialize)]
struct Explicit<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    consumer: &'a str,
    coordinate: &'a str,
    phase: &'static str,
    level: &'static str,
    message: &'a str,
    fields: BorrowedObject<'a>,
}

#[derive(Serialize)]
struct Terminal<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    consumer: &'a str,
    coordinate: &'a str,
    invocation: u64,
    outcome: ConsumerOutcome,
    cause: &'static str,
}

impl Logger {
    pub(crate) fn consumer_log(
        &self,
        consumer: &str,
        coordinate: &str,
        phase: &'static str,
        record: &HookLog,
    ) {
        self.enqueue(
            Kind::Hook,
            json(&Explicit {
                envelope: Envelope::new(None),
                kind: "consumer_log",
                consumer,
                coordinate,
                phase,
                level: record.level,
                message: &record.message,
                fields: BorrowedObject(&record.fields),
            }),
        );
    }

    pub(crate) fn consumer_terminal(
        &self,
        consumer: &str,
        coordinate: &str,
        invocation: u64,
        outcome: ConsumerOutcome,
        cause: &'static str,
    ) {
        self.enqueue(
            Kind::Subscription,
            json(&Terminal {
                envelope: Envelope::new(None),
                kind: "consumer_terminal",
                consumer,
                coordinate,
                invocation,
                outcome,
                cause,
            }),
        );
    }
}
