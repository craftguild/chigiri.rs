//! Bind the private Mock log sink to one fresh setup without exposing it to application code.

mod state;

use std::mem::size_of;
use std::sync::Arc;

use chigiri::__private::{HostSubscriptionObservation, SubscriptionObservationReceipt};
use chigiri::{FieldError, FieldResult, SubscriptionOverflow};

pub(super) use state::EntryFence;
use state::{Record, Shared};
pub(in crate::service) use state::{RegisteredSignal, RegistrationFence};

use super::http::Trace;
use crate::logging::SubscriptionSink;
use crate::luau::ScriptError;
use crate::publication::{CapturedEvent, RegisteredSubscription};

pub(in crate::service) struct EstablishedSubscription {
    registered: RegisteredSubscription,
    observation: Option<RegisteredSignal>,
}

pub(in crate::service) struct EstablishedParts<S> {
    pub(in crate::service) source: S,
    pub(in crate::service) overflow: SubscriptionOverflow,
    pub(in crate::service) observation: Option<SubscriptionObservationReceipt>,
}

struct Observation {
    shared: Arc<Shared>,
    sink: Option<SubscriptionSink>,
    coordinate: Box<str>,
    request_id: u64,
    retained_bytes: usize,
    dropped_total: u64,
}

pub(in crate::service) fn prepare(
    client: &super::super::Client,
    parent_type: &str,
    field_name: &str,
    receipt: SubscriptionObservationReceipt,
    remaining_metadata_bytes: usize,
) -> FieldResult<Option<Box<dyn HostSubscriptionObservation>>> {
    let Some(trace) = client.session.as_ref().map(|session| &session.trace) else {
        return Ok(None);
    };
    let sink = trace
        .subscription_logs
        .as_ref()
        .map(crate::logging::Logger::subscription_sink);
    let coordinate_len = parent_type
        .len()
        .checked_add(1)
        .and_then(|bytes| bytes.checked_add(field_name.len()))
        .ok_or_else(metadata_error)?;
    let retained_bytes = size_of::<Observation>()
        .checked_add(state::allocation_bytes())
        .and_then(|bytes| bytes.checked_add(size_of::<EntryFence>()))
        .and_then(|bytes| bytes.checked_add(size_of::<RegistrationFence>()))
        .and_then(|bytes| bytes.checked_add(size_of::<RegisteredSignal>()))
        .and_then(|bytes| bytes.checked_add(coordinate_len))
        .ok_or_else(metadata_error)?;
    if retained_bytes > remaining_metadata_bytes {
        return Err(metadata_error());
    }
    let mut coordinate = String::new();
    coordinate
        .try_reserve_exact(coordinate_len)
        .map_err(|_| metadata_error())?;
    coordinate.push_str(parent_type);
    coordinate.push('.');
    coordinate.push_str(field_name);
    let shared = Shared::new(receipt);
    trace
        .subscription_observation
        .set(shared.entry())
        .map_err(|_| invariant_error())?;
    Ok(Some(Box::new(Observation {
        shared,
        sink,
        coordinate: coordinate.into_boxed_str(),
        request_id: trace.id,
        retained_bytes,
        dropped_total: 0,
    })))
}

pub(super) fn enter(trace: &Trace) -> Result<Option<RegistrationFence>, ScriptError> {
    trace
        .subscription_observation
        .get()
        .map(EntryFence::enter)
        .transpose()
        .map_err(|_| ScriptError::new("source", "subscription observation entry closed"))
}

impl EstablishedSubscription {
    #[cfg(test)]
    pub(in crate::service) fn source_delivery_for_test(
        &self,
    ) -> Arc<chigiri::__private::HostSourceDelivery> {
        self.registered.source_delivery()
    }

    pub(in crate::service) fn new(
        registered: RegisteredSubscription,
        observation: Option<RegisteredSignal>,
    ) -> Self {
        Self {
            registered,
            observation,
        }
    }

    pub(in crate::service) fn into_parts(
        self,
    ) -> Result<
        EstablishedParts<
            impl futures_core::Stream<Item = Result<CapturedEvent, ScriptError>> + Send + 'static,
        >,
        ScriptError,
    > {
        let receipt = self
            .observation
            .map(RegisteredSignal::into_receipt)
            .transpose()
            .map_err(|_| {
                ScriptError::new("source", "subscription observation attachment closed")
            })?;
        let (source, overflow) = self.registered.into_parts();
        Ok(EstablishedParts {
            source,
            overflow,
            observation: receipt,
        })
    }
}

impl HostSubscriptionObservation for Observation {
    fn source_delivery(&self) -> Option<Arc<chigiri::__private::HostSourceDelivery>> {
        self.shared.source_delivery()
    }
    fn retained_metadata_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn flush(&mut self) {
        let records = self.shared.drain(self.dropped_total);
        for record in [records.first, records.second].into_iter().flatten() {
            self.emit(record);
        }
    }

    fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        self.shared.registered(overflow)
    }

    fn drop_oldest(&mut self, dropped_total: u64) {
        if let Some(generation) = self.shared.drop_oldest(dropped_total) {
            self.dropped_total = dropped_total;
            if let Some(sink) = &self.sink {
                sink.drop_oldest(generation, &self.coordinate, dropped_total);
            }
        }
    }

    fn finish(
        &mut self,
        decision: chigiri::__private::SubscriptionTermination,
        dropped_total: u64,
    ) {
        self.dropped_total = dropped_total;
        self.shared.finish(decision);
        self.flush();
    }
}

impl Observation {
    fn emit(&self, record: Record) {
        let Some(sink) = &self.sink else {
            return;
        };
        match record {
            Record::SetupRegistered {
                generation,
                overflow,
            } => sink.setup_registered(self.request_id, &self.coordinate, generation, overflow),
            Record::SetupUnregistered { cancelled } => {
                sink.setup_unregistered(self.request_id, &self.coordinate, cancelled)
            }
            Record::Terminal {
                generation,
                decision,
                dropped_total,
            } => sink.terminal(generation, &self.coordinate, decision, dropped_total),
        }
    }
}

fn metadata_error() -> FieldError {
    FieldError::internal(std::io::Error::other(
        "subscription observation metadata exceeds its bound",
    ))
}

fn invariant_error() -> FieldError {
    FieldError::internal(std::io::Error::other(
        "subscription observation was already installed",
    ))
}
