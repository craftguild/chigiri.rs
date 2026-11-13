//! Connect the production endpoint to its worker and verify hook progression plus HTTP observations.

mod after_work;
mod atomic;
mod budget;
mod concurrency;
mod dto;
mod guards;
mod late_jobs;
mod managed;
mod observation_budget;
mod panic_jobs;
mod phases;
mod preparation;
mod rejection_delivery;
mod rejection_order;
mod resolver_request;
pub(super) mod scheduling;
mod snapshots;
mod terminal;
mod time_warning;
mod timeleap;
mod timeleap_terminal;
mod warning_origins;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chigiri::{BoxFuture, DeadlineFactory, Endpoint, FieldError};
use chigiri_http::{
    ApplicationMetadata, BufferedBody, RequestHeader, ResponseParts, TransportRequest,
};

use crate::hooks::{HookSlot, StateContract};
use crate::luau::{Contract, HookCandidate, ScriptSet};
use crate::service::{Client, HookEvent, HttpScope, Worker};
use crate::store::Store;
use crate::value::ValueBudget;

const STATE: &str = r#"{"fields":{"count":{"kind":"integer"}},"initial":{"count":1}}"#;

struct Fixture {
    worker: Worker,
    endpoint: Endpoint<Client>,
    events: Arc<Mutex<Vec<HookEvent>>>,
}

impl Fixture {
    fn new(sdl: &str, seed: &str, resolvers: &[(&str, &str)], hooks: &[(HookSlot, &str)]) -> Self {
        Self::new_with_clock(
            sdl,
            seed,
            resolvers,
            hooks,
            crate::clock::ClockSettings::system(),
        )
    }

    fn new_with_clock(
        sdl: &str,
        seed: &str,
        resolvers: &[(&str, &str)],
        hooks: &[(HookSlot, &str)],
        clock: crate::clock::ClockSettings,
    ) -> Self {
        let ir = crate::service::tests::ir(sdl);
        let state = StateContract::from_json(STATE, &mut ValueBudget::default()).unwrap();
        let contract = Contract::with_state(ir.clone(), state).unwrap();
        let hook_candidates = hooks
            .iter()
            .map(|(slot, body)| {
                let body = if body.contains("return") {
                    (*body).to_owned()
                } else {
                    format!("{body}\nreturn nil")
                };
                let source = crate::luau::render_hook_module(*slot, &body).unwrap();
                let gate = crate::vm_gate::test_guard();
                let checked_source = format!("{}{}", crate::luau::checker_prelude(), source);
                let checked =
                    luaur::check_with_definitions(&checked_source, contract.declarations())
                        .map_err(|errors| format!("fixture {slot:?}: {errors:?}"));
                drop(gate);
                checked.unwrap();
                HookCandidate {
                    slot: *slot,
                    source,
                }
            })
            .collect();
        let hooks = contract.hooks(hook_candidates, None).unwrap();
        let candidates = crate::service::blog_fixture::candidates(sdl, &ir, resolvers);
        let scripts = ScriptSet::new(contract, candidates).unwrap();
        let worker = Worker::from_checked_with_clock(
            scripts,
            hooks,
            Store::from_seed_json(ir, seed).unwrap(),
            clock,
        )
        .unwrap();
        let endpoint = crate::service::endpoint(worker.client.clone(), true, |_, _| {
            FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
        })
        .unwrap();
        Self {
            worker,
            endpoint,
            events: Arc::default(),
        }
    }

    async fn request(
        &self,
        method: &str,
        query: Option<&str>,
        body: Vec<u8>,
        extra: &[RequestHeader<'_>],
    ) -> (ResponseParts, Option<u16>) {
        let events = self.events.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_events(move |event| events.lock().unwrap().push(event));
        let metadata = ApplicationMetadata::default();
        let mut headers = vec![
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("x-chigiri-preflight", b"1"),
        ];
        headers.extend_from_slice(extra);
        let request = TransportRequest::new(method, "/graphql", query, &headers, None, &metadata);
        let response = scope
            .run(
                self.endpoint
                    .handle(request, BufferedBody::new(body), &Clock),
            )
            .await;
        (response, scope.response_status())
    }

    async fn query(&self, query: &str) -> (serde_json::Value, Option<u16>) {
        let (response, status) = self
            .request(
                "POST",
                None,
                serde_json::to_vec(&serde_json::json!({"query":query})).unwrap(),
                &[],
            )
            .await;
        (serde_json::from_slice(&response.body).unwrap(), status)
    }

    fn logs(&self) -> Vec<(HookSlot, serde_json::Value)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|event| {
                let record = event
                    .record
                    .as_ref()
                    .unwrap_or_else(|error| panic!("{}: {error}", event.slot.name()));
                (
                    event.slot,
                    chigiri_http::value_to_json(&chigiri::Value::Object(record.fields.clone()))
                        .unwrap(),
                )
            })
            .collect()
    }

    async fn stop(mut self) {
        self.worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}

struct Clock;
impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
