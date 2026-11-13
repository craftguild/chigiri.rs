//! Exercise trusted facade setup with a real VM worker, quotas, and cleanup.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use chigiri::__streaming::{
    ActiveBridgeOperation, BridgeCleanupOutcome, BridgeOperationKind, EndpointSubscription,
    PreparedHttpSubscription, SubscriptionHttpIngress, SubscriptionInitialization,
    SubscriptionOutput, SubscriptionRuntimeGuard,
};
use chigiri::{BoxFuture, DeadlineFactory, Endpoint, GraphQlRequest};
use chigiri_http::{
    ApplicationMetadata, BufferedBody, RequestHeader, RequestMethod, ResponseCompletionOutcome,
    ResponseMediaType, ResponseParts, SubscriptionTransport, TransportRequest,
};

use crate::hooks::HookSlot;
use crate::ir::ScriptRole;
use crate::luau::{Contract, HookCandidate, ScriptSet};
use crate::service::{Client, HttpScope, ResolverDiagnostic, Worker};
use crate::store::Store;

const SDL: &str = r#"
schema { query: Query mutation: Mutation subscription: Signals }
type Query { ping: Int! }
type Mutation { publish(id: ID!): Item! edit(id: ID!, value: Int!): Item! clear: Int! fail: Int }
type Signals { changed(topic: String!): Item! }
type Item { id: ID! value: Int! current(mode: String! = ""): Int! }
"#;

pub(super) struct Clock;

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

pub(super) struct Fixture {
    pub(super) worker: Worker,
    pub(super) endpoint: Endpoint<Client>,
    pub(super) reports: Arc<AtomicUsize>,
    pub(super) resolver_diagnostics: Arc<Mutex<Vec<ResolverDiagnostic>>>,
    pub(super) hook_diagnostics: Arc<AtomicUsize>,
    subscription_logs: crate::logging::test_support::SubscriptionLogs,
}

mod scope;

impl Fixture {
    pub(super) fn new() -> Self {
        Self::build(&[], "return ctx.pubsub.subscribe.Item({topic=args.topic})")
    }

    pub(super) fn with_hooks(hooks: &[(HookSlot, &str)]) -> Self {
        Self::build(
            hooks,
            "return ctx.pubsub.subscribe.Item({topic=args.topic})",
        )
    }

    pub(super) fn with_subscription_body(body: &str) -> Self {
        Self::build(&[], body)
    }

    pub(super) fn without_subscription_logging(body: &str) -> Self {
        Self::build_with_logging(&[], body, false)
    }

    fn build(hooks: &[(HookSlot, &str)], subscription_body: &str) -> Self {
        Self::build_with_logging(hooks, subscription_body, true)
    }

    fn build_with_logging(
        hooks: &[(HookSlot, &str)],
        subscription_body: &str,
        logging: bool,
    ) -> Self {
        let ir = crate::service::tests::ir(SDL);
        let mut candidates = crate::service::tests::candidates(
            &ir,
            &[
                ("Query.ping", "return 1"),
                (
                    "Mutation.publish",
                    "local row=ctx.models.Item:create({id=args.id,value=1}) ctx.pubsub.publish.Item({topic='events',payload=row}) row=ctx.models.Item:update({where={field='id',op='eq',value=args.id}},{value=2})[1] ctx.pubsub.publish.Item({topic='events',payload=row}) return row",
                ),
                (
                    "Mutation.edit",
                    "return ctx.models.Item:update({where={field='id',op='eq',value=args.id}},{value=args.value})[1]",
                ),
                ("Mutation.fail", "error('private later-root failure')"),
                ("Mutation.clear", "return ctx.models.Item:delete({})"),
                (
                    "Item.current",
                    "if parent.value==1 then if args.mode=='business' then ctx.fail({code='DENIED',message='event denied'}) elseif args.mode=='internal' then error('private event failure') elseif args.mode=='timeout' then while true do end elseif args.mode=='local' then for _=1,10000 do ctx.models.Item:count({}) end end end return ctx.models.Item:find({where={field='id',op='eq',value=parent.id}})[1].value",
                ),
            ],
        );
        for (index, plan) in ir.scripts().iter().enumerate() {
            if matches!(plan.role, ScriptRole::SubscriptionRoot { .. }) {
                candidates[index].source = crate::luau::render_subscription_module(
                    &ir,
                    candidates[index].script,
                    subscription_body,
                    None,
                )
                .unwrap();
            }
        }
        let contract = Contract::new(ir.clone()).unwrap();
        for candidate in &candidates {
            let gate = crate::vm_gate::test_guard();
            let source = format!("{}{}", crate::luau::checker_prelude(), candidate.source);
            let checked = luaur::check_with_definitions(&source, contract.declarations());
            drop(gate);
            assert!(
                checked.is_ok(),
                "fixture {:?}: {checked:?}",
                candidate.script
            );
        }
        let hooks = hooks
            .iter()
            .map(|(slot, body)| HookCandidate {
                slot: *slot,
                source: crate::luau::render_hook_module(*slot, &format!("{body}\nreturn nil"))
                    .unwrap(),
            })
            .collect();
        let hooks = contract.hooks(hooks, None).unwrap();
        let worker = Worker::from_checked(
            ScriptSet::new(contract, candidates).unwrap(),
            hooks,
            Store::empty(ir),
        )
        .unwrap();
        let reports = Arc::new(AtomicUsize::new(0));
        let report_calls = reports.clone();
        let resolver_diagnostics = Arc::new(Mutex::new(Vec::new()));
        let resolver_calls = resolver_diagnostics.clone();
        let hook_diagnostics = Arc::new(AtomicUsize::new(0));
        let hook_calls = hook_diagnostics.clone();
        let subscription_logs = crate::logging::test_support::SubscriptionLogs::new();
        let subscription_logger = logging.then(|| subscription_logs.logger.clone());
        let endpoint = crate::service::endpoint_with_scopes(
            worker.client.clone(),
            false,
            move |_, _| {
                report_calls.fetch_add(1, Ordering::SeqCst);
                chigiri::FieldError::internal(std::io::Error::other("Mock resolver failed"))
            },
            move || {
                scope::build(
                    subscription_logger.clone(),
                    resolver_calls.clone(),
                    hook_calls.clone(),
                )
            },
        )
        .unwrap();
        Self {
            worker,
            endpoint,
            reports,
            resolver_diagnostics,
            hook_diagnostics,
            subscription_logs,
        }
    }

    pub(super) async fn subscribe(&self) -> Subscription {
        self.subscribe_query(
            "subscription { chosen: changed(topic:\"events\") { id value current } }",
        )
        .await
    }

    pub(super) async fn subscribe_query(&self, query: &str) -> Subscription {
        let active = self
            .endpoint
            .__begin_http_subscription()
            .unwrap()
            .activate(BridgeOperationKind::Subscription)
            .unwrap();
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("authorization", b"subscriber")];
        let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        // This component fixture is the trusted bridge caller. Protocol method,
        // origin, framing, worker supervision, and wire I/O are tested separately.
        let initialized = self
            .endpoint
            .__start_subscription(
                transport,
                GraphQlRequest::new(query),
                RequestMethod::Post,
                ResponseMediaType::GraphQlResponseJson,
                SubscriptionTransport::ServerSentEvents,
                None,
                &Clock,
            )
            .await;
        let SubscriptionInitialization::Ready {
            subscription,
            completion,
            ..
        } = initialized
        else {
            if let SubscriptionInitialization::Response { response, .. } = initialized {
                panic!("source setup failed: {response:?}");
            }
            panic!("source setup did not become ready")
        };
        let response = completion.finish(
            ResponseParts {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            },
            ResponseCompletionOutcome::Encoded,
        );
        assert_eq!(response.status, 200);
        Subscription {
            live: *subscription,
            active,
        }
    }

    pub(super) async fn execute(&self, query: &str) -> serde_json::Value {
        self.execute_with_logging(query, false).await
    }

    pub(super) async fn execute_logged(&self, query: &str) -> serde_json::Value {
        self.execute_with_logging(query, true).await
    }

    async fn execute_with_logging(
        &self,
        query: &str,
        subscription_logging: bool,
    ) -> serde_json::Value {
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("content-type", b"application/json")];
        let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let body = serde_json::to_vec(&serde_json::json!({"query":query})).unwrap();
        let mut scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
        if subscription_logging {
            scope = scope.with_subscription_logging(self.subscription_logs.logger.clone());
        }
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            scope.run(
                self.endpoint
                    .handle(transport, BufferedBody::new(body), &Clock),
            ),
        )
        .await
        .unwrap();
        serde_json::from_slice(&response.body).unwrap()
    }

    pub(super) async fn prepare_monitored(
        &self,
        query: &str,
    ) -> (PreparedHttpSubscription, SubscriptionRuntimeGuard) {
        let guard = self.endpoint.__register_subscription_runtime().unwrap();
        let metadata = ApplicationMetadata::default();
        let headers = [
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("accept", b"text/event-stream"),
        ];
        let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let body = serde_json::to_vec(&serde_json::json!({"query": query})).unwrap();
        let ingress = self
            .endpoint
            .__handle_http_streaming(transport, BufferedBody::new(body), &Clock)
            .await;
        let SubscriptionHttpIngress::Pending(pending) = ingress else {
            panic!("expected pending streaming input")
        };
        let prepared = pending.prepare().await;
        (prepared, guard)
    }

    pub(super) fn take_subscription_logs(&self) -> Vec<serde_json::Value> {
        self.subscription_logs.take()
    }

    pub(super) async fn stop(mut self) {
        self.worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}

pub(super) struct Subscription {
    pub(super) live: EndpointSubscription<Client>,
    active: ActiveBridgeOperation,
}

impl Subscription {
    pub(super) async fn close(self) {
        let terminal = self.active.revoke().unwrap();
        assert!(matches!(
            terminal.coordinate(self.live.into_cleanup(), &Clock).await,
            BridgeCleanupOutcome::Completed
        ));
    }
}

pub(super) async fn next(subscription: &mut EndpointSubscription<Client>) -> serde_json::Value {
    let output = output(subscription).await;
    let SubscriptionOutput::Event(response) = output else {
        panic!("source did not yield an event")
    };
    assert!(response.errors.is_empty(), "{response:?}");
    chigiri_http::value_to_json(&response.data.unwrap()).unwrap()
}

pub(super) async fn output(subscription: &mut EndpointSubscription<Client>) -> SubscriptionOutput {
    tokio::time::timeout(Duration::from_secs(8), subscription.next(&Clock))
        .await
        .unwrap()
}

pub(super) fn success(response: &serde_json::Value) {
    assert!(
        response
            .get("errors")
            .is_none_or(|errors| errors.as_array().unwrap().is_empty()),
        "{response}"
    );
}
