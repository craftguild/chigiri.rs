//! Build checked subscription modules and submit them through the real client.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use super::super::{Client, Worker};
use crate::cancellation::Cancellation;
use crate::ir::{FieldId, MockIr, ScriptId};
use crate::luau::{Contract, ScriptSet, static_candidates};
use crate::runtime::{Execution, ExecutionBudget, SubscriptionBinding};
use crate::store::Store;

const SDL: &str = r#"
schema { query: Query mutation: Mutation subscription: Subscription }
type Query { ping: Int }
type Mutation { change: Int }
type Item { id: ID! title: String! }
type Payload { value: String! }
type Subscription {
    presence(topic: String!, drop: Boolean!): String!
    count: Int!
    maybe: Int
    texts: [String!]!
    item: Item!
    dto: Payload!
}
"#;

pub(super) struct Fixture {
    pub(super) worker: Worker,
    pub(super) subscription: FieldId,
    pub(super) subscription_script: ScriptId,
    pub(super) query: FieldId,
    pub(super) mutation: FieldId,
}

impl Fixture {
    pub(super) fn new(body: &str) -> Self {
        Self::build(Some(body))
    }

    pub(super) fn generated() -> Self {
        Self::build(None)
    }

    fn build(subscription_body: Option<&str>) -> Self {
        Self::build_field(subscription_body, "presence", None)
    }

    pub(super) fn for_field(body: &str, field: &str, seed: Option<&str>) -> Self {
        Self::build_field(Some(body), field, seed)
    }

    fn build_field(subscription_body: Option<&str>, selected: &str, seed: Option<&str>) -> Self {
        let ir = crate::service::tests::ir(SDL);
        let contract = Contract::new(ir.clone()).unwrap();
        let subscription = field(&ir, "Subscription", selected);
        let query = field(&ir, "Query", "ping");
        let mutation = field(&ir, "Mutation", "change");
        let subscription_script = script(&ir, subscription);
        let mut candidates = static_candidates(&ir).unwrap();
        if let Some(body) = subscription_body {
            candidates[subscription_script.0].source =
                crate::luau::render_subscription_module(&ir, subscription_script, body, None)
                    .unwrap();
        }
        for (field, body) in [
            (query, ordinary_body("read")),
            (mutation, ordinary_body("mutation")),
        ] {
            let id = script(&ir, field);
            candidates[id.0].source = crate::luau::render_module(&ir, id, body).unwrap();
        }
        let scripts = ScriptSet::new(contract, candidates).unwrap();
        let store = match seed {
            Some(seed) => Store::from_seed_json(ir, seed).unwrap(),
            None => Store::empty(ir),
        };
        let worker = Worker::from_checked(scripts, Vec::new(), store).unwrap();
        Self {
            worker,
            subscription,
            subscription_script,
            query,
            mutation,
        }
    }

    pub(super) async fn setup(
        &self,
        arguments: Object,
    ) -> Result<SubscriptionBinding, crate::luau::ScriptError> {
        self.setup_with_budget(arguments, ExecutionBudget::default())
            .await
    }

    pub(super) async fn setup_with_budget(
        &self,
        arguments: Object,
        budget: ExecutionBudget,
    ) -> Result<SubscriptionBinding, crate::luau::ScriptError> {
        budget.set_clock(&self.worker.client.clock);
        let execution = Execution {
            state: self.worker.client.initial_state().snapshot(),
            request: None,
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Cancellation::default(),
            shutdown: self.worker.client.shutdown.clone(),
            budget,
        };
        self.worker
            .client
            .subscription_setup(self.subscription, arguments, None, execution)
            .await
    }
}

pub(super) fn arguments(topic: &str, drop: bool) -> Object {
    Object::from_iter([
        ("topic", Value::String(topic.to_owned())),
        ("drop", Value::Boolean(drop)),
    ])
}

pub(super) async fn stop(mut fixture: Fixture) {
    fixture
        .worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

pub(super) fn foreign_execution(client: &Client) -> Execution {
    Execution {
        state: client.initial_state().snapshot(),
        request: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget: ExecutionBudget::default(),
    }
}

fn field(ir: &Arc<MockIr>, parent: &str, name: &str) -> FieldId {
    ir.graph().types()[ir.graph().type_id(parent).unwrap().0].fields[name]
}

fn script(ir: &MockIr, field: FieldId) -> ScriptId {
    ScriptId(
        ir.scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap(),
    )
}

fn ordinary_body(scope: &str) -> &'static str {
    match scope {
        "read" => {
            "local dynamic: any = ctx assert(dynamic.pubsub == nil) assert(dynamic.generator == nil) assert(dynamic.subscription == nil) assert(dynamic.publish == nil) return 1"
        }
        _ => {
            "local dynamic: any = ctx assert(dynamic.pubsub == nil or dynamic.pubsub.subscribe == nil) assert(dynamic.generator == nil) assert(dynamic.subscription == nil) return 1"
        }
    }
}
