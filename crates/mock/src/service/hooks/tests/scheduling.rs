//! Pause existing plugin-chain boundaries for deterministic hook scheduling tests.

use super::*;
use chigiri::core::executor::ResolvedValue;
use chigiri::{
    FieldContext, FieldNext, FieldResult, GraphQlResponse, OperationContext, OperationNext, Plugin,
    RequestContext, RequestNext,
};
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Stage {
    BeforeOperation,
    AfterOperation,
    BeforeHook,
    BeforeResolver,
    AfterResolver,
    AfterHook,
    HookResult,
}

tokio::task_local! { static RESULT_GATE: Option<Arc<Pause>>; }

/// Pause after VM completion but before the session collects rejections and failures.
pub(in crate::service::hooks) async fn hook_result() {
    if let Ok(Some(pause)) = RESULT_GATE.try_with(Clone::clone) {
        pause.arrived.notify_one();
        tokio::time::timeout(Duration::from_secs(2), pause.released.notified())
            .await
            .expect("hook result release must succeed");
        pause.passed.notify_one();
    }
}

pub(super) struct Pause {
    alias: &'static str,
    stage: Stage,
    arrived: Notify,
    released: Notify,
    passed: Notify,
}

impl Pause {
    pub fn new(alias: &'static str, stage: Stage) -> Arc<Self> {
        Arc::new(Self {
            alias,
            stage,
            arrived: Notify::new(),
            released: Notify::new(),
            passed: Notify::new(),
        })
    }

    pub async fn reached(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.arrived.notified())
            .await
            .expect("the specified execution boundary must be reached");
    }

    pub async fn proceed(&self) {
        self.released.notify_one();
        tokio::time::timeout(Duration::from_secs(2), self.passed.notified())
            .await
            .expect("execution must cross the release boundary");
    }
}

pub(super) struct Schedule {
    pauses: Vec<Arc<Pause>>,
    pub completed: Arc<Mutex<Vec<String>>>,
}

impl Schedule {
    pub fn new(pauses: &[Arc<Pause>]) -> Self {
        Self {
            pauses: pauses.to_vec(),
            completed: Arc::default(),
        }
    }

    async fn wait(&self, alias: &str, stage: Stage) {
        if let Some(pause) = self
            .pauses
            .iter()
            .find(|pause| pause.alias == alias && pause.stage == stage)
        {
            pause.arrived.notify_one();
            tokio::time::timeout(Duration::from_secs(2), pause.released.notified())
                .await
                .expect("test releases execution boundary");
            pause.passed.notify_one();
        }
    }
}

static HOOKS: super::super::Hooks = super::super::Hooks;

impl Plugin<Client> for Schedule {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, Client>,
        next: RequestNext<'a, Client>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        HOOKS.request(context, next)
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, Client>,
        next: OperationNext<'a, Client>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let name = context.operation_name().unwrap_or("");
            self.wait(name, Stage::BeforeOperation).await;
            let response = HOOKS.operation(context, next).await;
            self.wait(name, Stage::AfterOperation).await;
            response
        })
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, Client>,
        next: FieldNext<'a, Client>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let alias = match context.path() {
                [chigiri::PathSegment::Field(alias)] => alias.as_ref(),
                _ => "",
            };
            self.wait(alias, Stage::BeforeHook).await;
            let gate = self
                .pauses
                .iter()
                .find(|pause| pause.alias == alias && pause.stage == Stage::HookResult)
                .cloned();
            let result = RESULT_GATE
                .scope(
                    gate,
                    HOOKS.field(
                        context,
                        FieldNext::new(&[], move |context| {
                            Box::pin(async move {
                                self.wait(alias, Stage::BeforeResolver).await;
                                let result = next.run(context).await;
                                self.wait(alias, Stage::AfterResolver).await;
                                result
                            })
                        }),
                    ),
                )
                .await;
            self.wait(alias, Stage::AfterHook).await;
            if !alias.is_empty() {
                self.completed.lock().unwrap().push(alias.into());
            }
            result
        })
    }
}

impl Fixture {
    pub fn scheduled(mut self, schedule: Schedule) -> Self {
        self.endpoint =
            crate::service::dispatch::scheduled_for_test(self.worker.client.clone(), schedule);
        self
    }
}
