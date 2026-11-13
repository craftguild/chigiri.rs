mod context;
mod dispatch;

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_http::{
    AcceptConnectionInit, ApplicationMetadata, RequestHeader, RequestMethod, RequestPhase,
    ResponseCompletionOutcome, ResponseMediaType, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::{ExecutableSchema, GraphQlRequest, Object, Value};

use self::context::{AppContext, Factory, Hook, RequestIds};
use self::dispatch::Dispatch;
use super::super::deadlines::Deadlines;
use super::super::descriptor::SCHEMA;
use super::super::harness::{block_on, response_parts};
use crate::subscription::driver::{
    EndpointSubscription, SubscriptionInitialization, SubscriptionOutput, start,
};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::subscription::state::SubscriptionState;
use crate::transport::{
    EndpointState, NeutralEndpoint, TransportSettings, erase_factory, erase_hook,
};

const METADATA_LIMIT: usize = 16 * 1024;

pub(super) struct Fixture {
    probe: Arc<Probe>,
    endpoint: Arc<EndpointState<AppContext>>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let probe = Arc::new(Probe::new());
        let schema =
            ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(Arc::clone(&probe)))
                .request_id_factory(RequestIds(Arc::clone(&probe)))
                .build()
                .unwrap();
        let endpoint = NeutralEndpoint::new(
            schema,
            chigiri_http::HttpLifecycle::new(),
            erase_factory(Factory(Arc::clone(&probe))),
            Some(erase_hook(Hook(Arc::clone(&probe)))),
            TransportSettings {
                body_limit: 1024,
                response_byte_limit: 4096,
                body_read_timeout: Duration::from_secs(1),
                transport_admission_timeout: Duration::from_secs(1),
                source_resolution_timeout: Duration::from_secs(1),
                decoded_request_timeout: Duration::from_secs(1),
                context_timeout: Duration::from_secs(1),
                execution_timeout: Duration::from_secs(1),
                response_hook_timeout: Duration::from_secs(1),
                request_timeout: Duration::from_secs(1),
            },
            SubscriptionState::new(
                ValidatedSubscriptionSettings::new(
                    SubscriptionLimits {
                        max_retained_metadata_bytes: METADATA_LIMIT,
                        ..SubscriptionLimits::default()
                    },
                    SubscriptionProtocolSettings::default(),
                ),
                Arc::new(AcceptConnectionInit),
            )
            .unwrap(),
        )
        .shared_state();
        Self { probe, endpoint }
    }

    pub(super) fn subscribe(
        &self,
        token: &str,
        limit: i32,
        events: impl IntoIterator<Item = i32>,
    ) -> Live {
        self.probe
            .sources
            .lock()
            .unwrap()
            .push_back(events.into_iter().collect());
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("authorization", token.as_bytes())];
        let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let mut variables = Object::new();
        variables.insert("limit", limit);
        let request = GraphQlRequest::new(
            "subscription Named($limit: Int!) { chosen: events(limit: $limit) { value } }",
        )
        .with_operation_name("Named")
        .with_variables(variables);
        let initialized = block_on(start(
            Arc::clone(&self.endpoint),
            transport,
            request,
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
            SubscriptionTransport::ServerSentEvents,
            None,
            &Deadlines(Vec::new()),
            None,
        ));
        let SubscriptionInitialization::Ready {
            subscription,
            completion,
            ..
        } = initialized
        else {
            panic!("authorized subscription did not initialize");
        };
        let _ = completion.finish(response_parts(), ResponseCompletionOutcome::Encoded);
        Live(*subscription)
    }

    pub(super) fn revoke(&self, token: &str) {
        self.probe.allowed.lock().unwrap().remove(token);
        self.probe.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn permit_limit(&self, limit: i32) {
        self.probe.allowed_limit.store(limit, Ordering::SeqCst);
        self.probe.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        let records = self.probe.records.lock().unwrap();
        Snapshot {
            credentials: records.credentials.clone(),
            setup_tokens: records.setup_tokens.clone(),
            request_ids: records.request_ids.clone(),
            phases: records.phases.clone(),
            root_arguments: records.root_arguments.clone(),
            policy_revisions: records.policy_revisions.clone(),
            local_starts: records.local_starts.clone(),
            nested_values: records.nested_values.clone(),
            setup_calls: self.probe.setup_calls.load(Ordering::SeqCst),
            source_polls: self.probe.source_polls.load(Ordering::SeqCst),
            source_drops: self.probe.source_drops.load(Ordering::SeqCst),
            mapper_calls: self.probe.mapper_calls.load(Ordering::SeqCst),
            hook_calls: self.probe.hook_calls.load(Ordering::SeqCst),
            completion_calls: self.probe.completion_calls.load(Ordering::SeqCst),
            context_drops: self.probe.context_drops.load(Ordering::SeqCst),
        }
    }
}

pub(super) struct Live(EndpointSubscription<AppContext>);

impl Live {
    pub(super) fn next(&mut self) -> SubscriptionOutput {
        block_on(self.0.next(&Deadlines(Vec::new())))
    }
}

pub(super) struct Snapshot {
    pub(super) credentials: Vec<String>,
    pub(super) setup_tokens: Vec<String>,
    pub(super) request_ids: Vec<String>,
    pub(super) phases: Vec<RequestPhase>,
    pub(super) root_arguments: Vec<i32>,
    pub(super) policy_revisions: Vec<usize>,
    pub(super) local_starts: Vec<(usize, usize, bool)>,
    pub(super) nested_values: Vec<i32>,
    pub(super) setup_calls: usize,
    pub(super) source_polls: usize,
    pub(super) source_drops: usize,
    pub(super) mapper_calls: usize,
    pub(super) hook_calls: usize,
    pub(super) completion_calls: usize,
    pub(super) context_drops: usize,
}

pub(super) fn event_value(output: &SubscriptionOutput) -> Option<i128> {
    let SubscriptionOutput::Event(response) = output else {
        return None;
    };
    let Value::Object(root) = response.data.as_ref()? else {
        return None;
    };
    let Value::Object(event) = root.get("chosen")? else {
        return None;
    };
    let Value::Int(value) = event.get("value")? else {
        return None;
    };
    Some(*value)
}

pub(super) fn error_code(response: &chigiri_runtime::GraphQlResponse) -> Option<&str> {
    let Value::String(code) = response.errors.first()?.extensions.get("code")? else {
        return None;
    };
    Some(code)
}

struct Probe {
    allowed: Mutex<BTreeSet<String>>,
    allowed_limit: AtomicI32,
    revision: AtomicUsize,
    sources: Mutex<VecDeque<Vec<i32>>>,
    records: Mutex<Records>,
    setup_calls: AtomicUsize,
    source_polls: AtomicUsize,
    source_drops: AtomicUsize,
    mapper_calls: AtomicUsize,
    hook_calls: AtomicUsize,
    completion_calls: AtomicUsize,
    context_drops: AtomicUsize,
    request_ids: AtomicUsize,
}

impl Probe {
    fn new() -> Self {
        Self {
            allowed: Mutex::new(BTreeSet::from(["token-a".into(), "token-b".into()])),
            allowed_limit: AtomicI32::new(2),
            revision: AtomicUsize::new(0),
            sources: Mutex::new(VecDeque::new()),
            records: Mutex::new(Records::default()),
            setup_calls: AtomicUsize::new(0),
            source_polls: AtomicUsize::new(0),
            source_drops: AtomicUsize::new(0),
            mapper_calls: AtomicUsize::new(0),
            hook_calls: AtomicUsize::new(0),
            completion_calls: AtomicUsize::new(0),
            context_drops: AtomicUsize::new(0),
            request_ids: AtomicUsize::new(0),
        }
    }
}

#[derive(Default)]
struct Records {
    credentials: Vec<String>,
    setup_tokens: Vec<String>,
    request_ids: Vec<String>,
    phases: Vec<RequestPhase>,
    root_arguments: Vec<i32>,
    policy_revisions: Vec<usize>,
    local_starts: Vec<(usize, usize, bool)>,
    nested_values: Vec<i32>,
}
