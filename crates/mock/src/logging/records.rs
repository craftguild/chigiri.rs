//! Keep user fields separate from envelopes and serialize them under a byte limit without building a second large DOM.

use std::time::{SystemTime, UNIX_EPOCH};

use chigiri::PathSegment;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

use super::{FORMAT, Loss, RECORD_BYTES};
use crate::service::HookEvent;
use crate::value::{
    ValueError,
    serialize::{BorrowedObject, render},
};

#[derive(Serialize)]
pub(super) struct Envelope {
    format: &'static str,
    timestamp: i128,
    request_id: Option<u64>,
}
impl Envelope {
    pub(super) fn new(request_id: Option<u64>) -> Self {
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(time) => time.as_millis() as i128,
            Err(error) => -(error.duration().as_millis() as i128),
        };
        Self {
            format: FORMAT,
            timestamp,
            request_id,
        }
    }
}

#[derive(Serialize)]
struct Hook<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    phase: crate::hooks::HookPhase,
    timing: crate::hooks::HookTiming,
    level: &'static str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinate: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Path<'a>>,
    fields: BorrowedObject<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<Diagnostic<'a>>,
}
#[derive(Serialize)]
struct Diagnostic<'a> {
    stack: Option<&'a crate::luau::ScriptStack>,
    module: Option<&'a str>,
    phase: &'static str,
    cause: &'static str,
    member: Option<&'a str>,
    expected: Option<&'a str>,
    actual: Option<&'a str>,
    hint: Option<&'a str>,
    line: Option<usize>,
    column: Option<usize>,
}
struct Path<'a>(&'a [PathSegment]);
impl Serialize for Path<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for part in self.0 {
            match part {
                PathSegment::Field(name) => values.serialize_element(name)?,
                PathSegment::Index(index) => values.serialize_element(index)?,
            }
        }
        values.end()
    }
}

pub(super) fn hook(event: &HookEvent) -> Result<Vec<u8>, ValueError> {
    let empty = chigiri::core::Object::new();
    let (level, message, fields, diagnostic) = match &event.record {
        Ok(log) => {
            if !crate::api::hooks::LEVELS.contains(&log.level) {
                return Err(ValueError("invalid log level"));
            }
            (log.level, log.message.as_str(), &log.fields, None)
        }
        Err(error) => (
            "error",
            "Mock hook failed",
            &empty,
            Some(Diagnostic {
                stack: error.stack.as_ref(),
                module: event.module.as_deref(),
                phase: error.phase,
                cause: error.cause,
                member: error.member.as_deref(),
                expected: some(&error.expected),
                actual: some(&error.actual),
                hint: some(&error.hint),
                line: error.source_position().map(|position| position.0),
                column: error.source_position().map(|position| position.1),
            }),
        ),
    };
    json(&Hook {
        envelope: Envelope::new(Some(event.request_id)),
        kind: "hook",
        phase: event.slot.phase(),
        timing: event.slot.timing(),
        level,
        message,
        coordinate: event.coordinate.as_deref(),
        path: (event.slot.phase() == crate::hooks::HookPhase::Field).then_some(Path(&event.path)),
        fields: BorrowedObject(fields),
        diagnostic,
    })
}

fn some(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

#[derive(Serialize)]
struct Access<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    method: &'a str,
    path: &'a str,
    status: Option<u16>,
    duration_ms: f64,
    outcome: &'static str,
}
pub(super) fn access(
    id: u64,
    method: &str,
    path: &str,
    status: Option<u16>,
    duration: std::time::Duration,
    completed: bool,
) -> Result<Vec<u8>, ValueError> {
    json(&Access {
        envelope: Envelope::new(Some(id)),
        kind: "access",
        method,
        path,
        status,
        duration_ms: duration.as_secs_f64() * 1000.0,
        outcome: if completed { "completed" } else { "cancelled" },
    })
}

#[derive(Serialize)]
struct LossRecord {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    dropped: Loss,
}
pub(super) fn loss(loss: Loss) -> Result<Vec<u8>, ValueError> {
    json(&LossRecord {
        envelope: Envelope::new(None),
        kind: "log_loss",
        dropped: loss,
    })
}

pub(super) fn notice(text: &str) -> Result<Vec<u8>, ValueError> {
    // Reuse JSON string escaping for human-readable stderr, omitting the surrounding quotes.
    let escaped = render(&text, RECORD_BYTES - 1)?;
    let mut bytes = escaped.as_bytes()[1..escaped.len() - 1].to_vec();
    bytes.push(b'\n');
    Ok(bytes)
}
pub(super) fn json(value: &impl Serialize) -> Result<Vec<u8>, ValueError> {
    let mut bytes = render(value, RECORD_BYTES - 1)?.into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Serialize)]
struct Resolver<'a> {
    format: &'static str,
    kind: &'static str,
    #[serde(flatten)]
    diagnostic: &'a crate::service::ResolverDiagnostic,
}

pub(super) fn resolver(event: &crate::service::ResolverDiagnostic) -> Result<Vec<u8>, ValueError> {
    json(&Resolver {
        format: FORMAT,
        kind: "resolver_diagnostic",
        diagnostic: event,
    })
}

#[derive(Serialize)]
struct LimitWarning<'a> {
    phase: &'static str,
    coordinate: Option<&'a str>,
    path: Path<'a>,
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    level: &'static str,
    limit: &'static str,
    soft: usize,
    hard: usize,
    observed: usize,
}

pub(super) fn limit_warning(
    id: u64,
    notice: crate::limits::warning::LimitWarning,
) -> Result<Vec<u8>, ValueError> {
    let policy = crate::limits::request::policy(notice.kind);
    json(&LimitWarning {
        phase: notice.origin.phase,
        coordinate: notice.origin.coordinate.as_deref(),
        path: Path(&notice.origin.path),
        envelope: Envelope::new(Some(id)),
        kind: "limit_warning",
        level: "warn",
        limit: policy.name,
        soft: policy.soft,
        hard: policy.hard,
        observed: notice.observed,
    })
}

#[derive(Serialize)]
struct Terminal<'a> {
    format: &'static str,
    kind: &'static str,
    #[serde(flatten)]
    diagnostic: &'a crate::service::TerminalDiagnostic,
}

pub(super) fn terminal(event: &crate::service::TerminalDiagnostic) -> Result<Vec<u8>, ValueError> {
    json(&Terminal {
        format: FORMAT,
        kind: "request_terminal",
        diagnostic: event,
    })
}
