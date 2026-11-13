//! Bounded carrier access and sanitized standard OpenTelemetry reconstruction.

use std::str::FromStr;

use chigiri_runtime::instrumentation::{
    PropagationCarrier, PropagationContext, PropagationField, TraceParent,
};
use opentelemetry::Context;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use opentelemetry_sdk::propagation::TraceContextPropagator;

const MAX_PARENT_BYTES: usize = 512;
const MAX_STATE_BYTES: usize = 512;
const MAX_STATE_MEMBERS: usize = 32;

pub(super) fn capture(carrier: &dyn PropagationCarrier) -> PropagationContext {
    let current = Context::current();
    // This control is selected even when current has no valid span.
    let suppressed = current.is_telemetry_suppressed();
    let parent = current.span();
    if parent.span_context().is_valid() {
        return snapshot(parent.span_context(), suppressed);
    }
    let fields = BoundedFields::read(carrier);
    let extracted = TraceContextPropagator::new().extract_with_context(&Context::new(), &fields);
    snapshot(extracted.span().span_context(), suppressed)
}

pub(super) fn snapshot(parent: &SpanContext, suppressed: bool) -> PropagationContext {
    let state = bounded_state(parent.trace_state());
    PropagationContext::new(
        TraceParent::new(
            parent.trace_id().to_bytes(),
            parent.span_id().to_bytes(),
            parent.trace_flags().to_u8(),
            parent.is_remote(),
            &state,
        ),
        suppressed,
    )
}

fn bounded_state(state: &TraceState) -> String {
    let entries = state.into_iter();
    if entries.len() > MAX_STATE_MEMBERS {
        return String::new();
    }
    let mut size = 0usize;
    for (index, (key, value)) in state.into_iter().enumerate() {
        let Some(next) = size
            .checked_add(key.len())
            .and_then(|size| size.checked_add(value.len()))
            .and_then(|size| size.checked_add(1 + usize::from(index != 0)))
        else {
            return String::new();
        };
        if next > MAX_STATE_BYTES {
            return String::new();
        }
        size = next;
    }
    let mut output = String::with_capacity(size);
    for (index, (key, value)) in state.into_iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(key);
        output.push('=');
        output.push_str(value);
    }
    output
}

pub(super) fn span_context(parent: &TraceParent) -> SpanContext {
    SpanContext::new(
        TraceId::from_bytes(parent.trace_id()),
        SpanId::from_bytes(parent.span_id()),
        TraceFlags::new(parent.flags()),
        parent.is_remote(),
        TraceState::from_str(parent.trace_state()).unwrap_or_default(),
    )
}

pub(super) fn context(values: &PropagationContext) -> Context {
    let context = values.parent().map_or_else(Context::new, |parent| {
        Context::new().with_remote_span_context(span_context(parent))
    });
    with_suppression(context, values.is_suppressed())
}

pub(super) fn normalize(values: &PropagationContext) -> PropagationContext {
    // Retain canonical values without caching the parser's owned string entries.
    let Some(parent) = values
        .parent()
        .filter(|parent| !parent.trace_state().is_empty())
    else {
        return values.clone();
    };
    let parsed = TraceState::from_str(parent.trace_state()).unwrap_or_default();
    let state = bounded_state(&parsed);
    if state == parent.trace_state() {
        return values.clone();
    }
    PropagationContext::new(
        TraceParent::new(
            parent.trace_id(),
            parent.span_id(),
            parent.flags(),
            parent.is_remote(),
            &state,
        ),
        values.is_suppressed(),
    )
}

pub(super) fn with_suppression(context: Context, suppressed: bool) -> Context {
    if suppressed {
        context.with_telemetry_suppressed()
    } else {
        context
    }
}

#[derive(Default)]
struct BoundedFields {
    parent: Option<String>,
    state: Option<String>,
}

impl BoundedFields {
    fn read(carrier: &dyn PropagationCarrier) -> Self {
        let mut fields = Self::default();
        let mut seen_parent = false;
        let mut invalid_parent = false;
        carrier.values(PropagationField::TraceParent, &mut |value| {
            if seen_parent || value.len() > MAX_PARENT_BYTES || !http_field_bytes(value) {
                invalid_parent = true;
                fields.parent = None;
            } else if !invalid_parent {
                fields.parent = std::str::from_utf8(value).ok().map(str::to_owned);
            }
            seen_parent = true;
        });
        let mut state = String::new();
        let mut seen_state = false;
        let mut invalid_state = false;
        carrier.values(PropagationField::TraceState, &mut |value| {
            if invalid_state {
                return;
            }
            let separator = usize::from(seen_state);
            if state.len() + separator > MAX_STATE_BYTES
                || value.len() > MAX_STATE_BYTES.saturating_sub(state.len() + separator)
                || !http_field_bytes(value)
            {
                invalid_state = true;
                return;
            }
            if seen_state {
                state.push(',');
            }
            state.push_str(std::str::from_utf8(value).expect("checked ASCII"));
            seen_state = true;
        });
        if !invalid_state {
            let members = state.split(',').map(str::trim).filter(|s| !s.is_empty());
            if members.clone().count() <= MAX_STATE_MEMBERS {
                // Normalize HTTP list whitespace; semantic parsing stays upstream.
                fields.state = Some(members.collect::<Vec<_>>().join(","));
            }
        }
        fields
    }
}

fn http_field_bytes(value: &[u8]) -> bool {
    value.iter().all(|byte| matches!(byte, b'\t' | b' '..=b'~'))
}

impl Extractor for BoundedFields {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.parent.as_deref(),
            "tracestate" => self.state.as_deref(),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        vec!["traceparent", "tracestate"]
    }
}
