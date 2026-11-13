//! A normal generated resolver owns its outgoing client and standard carriers.

use std::collections::HashMap;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use opentelemetry::Context;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{SpanContext, TraceContextExt};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Instrument;

use super::super::super::generated;
use super::super::super::support::{Item, ItemFields, MutationFields, Types};
use super::super::PrivateValue;

pub(super) struct Seen {
    pub(super) execution: SpanContext,
    pub(super) application: SpanContext,
}

struct Query {
    address: SocketAddr,
    seen: Arc<Mutex<Option<Seen>>>,
}

impl generated::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        let current = Context::current();
        assert!(current.get::<PrivateValue>().is_none());
        let execution = current.span().span_context().clone();
        let span = tracing::info_span!("application.resolver");
        async {
            let current = Context::current();
            let application = current.span().span_context().clone();
            assert_ne!(application.span_id(), execution.span_id());
            let mut carrier = HashMap::<String, String>::new();
            TraceContextPropagator::new().inject_context(&current, &mut carrier);
            assert!(
                carrier
                    .keys()
                    .all(|key| key == "traceparent" || key == "tracestate")
            );
            let mut headers = String::new();
            for (key, value) in carrier {
                write!(headers, "{key}: {value}\r\n").expect("writing to a String cannot fail");
            }
            let response =
                super::client::request_with_headers(self.address, "/downstream", None, &headers)
                    .await
                    .unwrap();
            assert_eq!(response.status, hyper::StatusCode::OK);
            assert_eq!(response.body, b"ok");
            assert_eq!(*Context::current().span().span_context(), application);
            assert!(
                self.seen
                    .lock()
                    .unwrap()
                    .replace(Seen {
                        execution,
                        application
                    })
                    .is_none()
            );
            Ok("interoperable".to_owned())
        }
        .instrument(span)
        .await
    }

    async fn item<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> chigiri::FieldResult<Item> {
        Ok(Item {
            id: "item-1".into(),
            name: "Chigiri".to_owned(),
        })
    }
}

pub(super) fn resolvers(
    address: SocketAddr,
    seen: Arc<Mutex<Option<Seen>>>,
) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query { address, seen })
        .with_mutation(MutationFields)
        .with_item(ItemFields)
        .build()
}
