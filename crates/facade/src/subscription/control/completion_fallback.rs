//! Precharged, fixed callback inputs and bounded initial-response accounting.

use chigiri_http::{ResponseCompletionOutcome, ResponseHeader, ResponseMediaType, ResponseParts};

use super::SubscriptionControlSignal;
use crate::subscription::delivery::{EncodeReservation, EndpointByteOwner, EventByteCharge};

pub(crate) const MAX_INITIAL_RESPONSE_BYTES: usize = 64 * 1024;
const HEADER_ENTRY_BYTES: usize = 16;
const FAILED_BODY: &[u8] = br#"{"errors":[{"message":"Internal server error","extensions":{"code":"INTERNAL_SERVER_ERROR"}}]}"#;
const TIMED_OUT_BODY: &[u8] =
    br#"{"errors":[{"message":"Request timed out","extensions":{"code":"REQUEST_TIMEOUT"}}]}"#;

/// Task-side callback inputs; no physical resource token enters the source task.
pub(crate) struct InitialCompletionFallbacks {
    failed: Option<ResponseParts>,
    timed_out: Option<ResponseParts>,
}

impl InitialCompletionFallbacks {
    pub(crate) fn take(
        &mut self,
        outcome: ResponseCompletionOutcome,
    ) -> Result<ResponseParts, SubscriptionControlSignal> {
        let input = match outcome {
            ResponseCompletionOutcome::Failed => &mut self.failed,
            ResponseCompletionOutcome::TimedOut => &mut self.timed_out,
            _ => return Err(SubscriptionControlSignal::InvalidTransportArguments),
        };
        input.take().ok_or(SubscriptionControlSignal::Accounting)
    }

    /// Releases unused fixed inputs before publishing a normal callback result.
    pub(crate) fn clear(&mut self) {
        self.failed.take();
        self.timed_out.take();
    }
}

/// Outer-owned exact charges for both separately prebuilt callback candidates.
pub(crate) struct InitialCompletionFallbackCharge {
    _failed: EventByteCharge,
    _timed_out: EventByteCharge,
}

/// One preparation's fallback charges and maximum callback-result reservation.
pub(crate) struct InitialPreparationByteGuards {
    fallbacks: InitialCompletionFallbackCharge,
    buffer: InitialResponseBufferGuard,
}

impl InitialPreparationByteGuards {
    /// Separates task-cleanup charges from the independently returned wire value.
    pub(crate) fn into_parts(
        self,
    ) -> (InitialCompletionFallbackCharge, InitialResponseBufferGuard) {
        (self.fallbacks, self.buffer)
    }
}

/// Outer-owned maximum reservation for one callback result or finite wire value.
pub(crate) struct InitialResponseBufferGuard {
    buffer: EncodeReservation,
}

impl InitialResponseBufferGuard {
    /// Shrinks only after receiving the task's checked result or selecting a fallback.
    #[cfg(test)]
    pub(crate) fn finish(
        mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionControlSignal> {
        self.try_finish(response)
    }

    /// A failed validation or shrink leaves the maximum token with this owner.
    pub(crate) fn try_finish(
        &mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionControlSignal> {
        let actual = initial_response_charge(response)?;
        self.buffer
            .try_commit(actual)
            .map_err(SubscriptionControlSignal::from_reservation)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn retains_maximum(&self) -> bool {
        self.buffer.retains(MAX_INITIAL_RESPONSE_BYTES)
    }
}

/// Reserves all initial callback storage before Phase A or application code.
pub(crate) fn reserve_initial_preparation(
    bytes: &EndpointByteOwner,
    media: ResponseMediaType,
) -> Result<(InitialCompletionFallbacks, InitialPreparationByteGuards), SubscriptionControlSignal> {
    let buffer = bytes
        .try_reserve(MAX_INITIAL_RESPONSE_BYTES)
        .map_err(SubscriptionControlSignal::from_reservation)?;
    let (fallbacks, charge) = reserve_initial_completion_fallbacks(bytes, media)?;
    Ok((
        fallbacks,
        InitialPreparationByteGuards {
            fallbacks: charge,
            buffer: InitialResponseBufferGuard { buffer },
        },
    ))
}

pub(crate) fn reserve_initial_completion_fallbacks(
    bytes: &EndpointByteOwner,
    media: ResponseMediaType,
) -> Result<(InitialCompletionFallbacks, InitialCompletionFallbackCharge), SubscriptionControlSignal>
{
    let failed_bytes = fixed_charge(media, FAILED_BODY)?;
    let timed_out_bytes = fixed_charge(media, TIMED_OUT_BODY)?;
    // Both reservations precede every response Vec/String allocation.
    let failed = bytes
        .try_reserve(failed_bytes)
        .map_err(SubscriptionControlSignal::from_reservation)?;
    let timed_out = bytes
        .try_reserve(timed_out_bytes)
        .map_err(SubscriptionControlSignal::from_reservation)?;
    let charge = InitialCompletionFallbackCharge {
        _failed: failed
            .commit(failed_bytes)
            .map_err(SubscriptionControlSignal::from_reservation)?,
        _timed_out: timed_out
            .commit(timed_out_bytes)
            .map_err(SubscriptionControlSignal::from_reservation)?,
    };
    Ok((
        InitialCompletionFallbacks {
            failed: Some(fixed_response(media, 500, FAILED_BODY)),
            timed_out: Some(fixed_response(media, 504, TIMED_OUT_BODY)),
        },
        charge,
    ))
}

/// Checks body and header storage before a callback result enters its port.
pub(crate) fn initial_response_charge(
    response: &ResponseParts,
) -> Result<usize, SubscriptionControlSignal> {
    let mut size = response.body.len();
    for header in &response.headers {
        size = size
            .checked_add(HEADER_ENTRY_BYTES)
            .and_then(|size| size.checked_add(header.name.len()))
            .and_then(|size| size.checked_add(header.value.len()))
            .ok_or(SubscriptionControlSignal::Accounting)?;
    }
    check_size(size)
}

fn fixed_charge(media: ResponseMediaType, body: &[u8]) -> Result<usize, SubscriptionControlSignal> {
    let size = body
        .len()
        .checked_add(HEADER_ENTRY_BYTES)
        .and_then(|size| size.checked_add("content-type".len()))
        .and_then(|size| size.checked_add(media.content_type().len()))
        .ok_or(SubscriptionControlSignal::Accounting)?;
    check_size(size)
}

fn check_size(size: usize) -> Result<usize, SubscriptionControlSignal> {
    if size > MAX_INITIAL_RESPONSE_BYTES {
        Err(SubscriptionControlSignal::ControlLimit)
    } else {
        Ok(size)
    }
}

fn fixed_response(media: ResponseMediaType, status: u16, body: &[u8]) -> ResponseParts {
    ResponseParts {
        status,
        headers: vec![ResponseHeader {
            name: "content-type".to_owned(),
            value: media.content_type().to_owned(),
        }],
        body: body.to_vec(),
    }
}

#[cfg(test)]
#[path = "tests/completion_fallback.rs"]
mod tests;
