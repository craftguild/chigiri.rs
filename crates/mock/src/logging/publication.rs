//! Serialize only schema coordinates and committed staging decisions, never payloads or topics.

use serde::Serialize;

use super::records::{Envelope, json};
use crate::publication::{MAX_PUBLICATIONS, MAX_SAFE_SEQUENCE};
use crate::value::ValueError;

#[derive(Serialize)]
struct Publication<'a> {
    #[serde(flatten)]
    envelope: Envelope,
    kind: &'static str,
    sequence: u64,
    index: u32,
    coordinate: &'a str,
    outcome: &'static str,
}

pub(super) fn record(
    id: u64,
    sequence: u64,
    index: usize,
    coordinate: &str,
    lost: bool,
) -> Result<Vec<u8>, ValueError> {
    if sequence == 0 || sequence > MAX_SAFE_SEQUENCE || index >= MAX_PUBLICATIONS {
        return Err(ValueError("invalid publication log identity"));
    }
    let index = u32::try_from(index).map_err(|_| ValueError("invalid publication log index"))?;
    json(&Publication {
        envelope: Envelope::new(Some(id)),
        kind: "subscription_publication",
        sequence,
        index,
        coordinate,
        outcome: if lost { "lost" } else { "staged" },
    })
}
