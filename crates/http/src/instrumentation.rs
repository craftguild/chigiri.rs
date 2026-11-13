//! Closed borrowed carrier forwarding shared by both native adapters.

use chigiri_runtime::instrumentation::{PropagationCarrier, PropagationField};

use crate::TransportRequest;

impl PropagationCarrier for TransportRequest<'_> {
    fn values(&self, field: PropagationField, visitor: &mut dyn FnMut(&[u8])) {
        let name = match field {
            PropagationField::TraceParent => "traceparent",
            PropagationField::TraceState => "tracestate",
        };
        for header in self.headers() {
            if header.name.eq_ignore_ascii_case(name) {
                visitor(header.value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApplicationMetadata, RequestHeader};

    #[test]
    fn carrier_visits_only_the_two_closed_fields_in_transport_order() {
        let headers = [
            RequestHeader::new("authorization", b"private"),
            RequestHeader::new("TraceParent", b"first"),
            RequestHeader::new("tracestate", b"one=value"),
            RequestHeader::new("traceparent", b"second"),
            RequestHeader::new("TraceState", b"two=value"),
            RequestHeader::new("baggage", b"private"),
        ];
        let metadata = ApplicationMetadata::default();
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let mut parents = Vec::new();
        request.values(PropagationField::TraceParent, &mut |value| {
            parents.push(value.to_vec())
        });
        assert_eq!(parents, [b"first".to_vec(), b"second".to_vec()]);
        let mut state = Vec::new();
        request.values(PropagationField::TraceState, &mut |value| {
            state.push(value.to_vec())
        });
        assert_eq!(state, [b"one=value".to_vec(), b"two=value".to_vec()]);
    }
}
