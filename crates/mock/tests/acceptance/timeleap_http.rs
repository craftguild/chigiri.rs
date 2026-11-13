//! In-process host control between real loopback HTTP requests.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::Source;
use chigiri::graphiql::Paths;
use chigiri_mock::hooks::HookSlot;
use chigiri_mock::ir::{MockIr, ScriptId};
use chigiri_mock::luau::{
    Contract, HookCandidate, ScriptCandidate, ScriptSet, render_hook_module, render_module,
};
use chigiri_mock::service::{HttpScope, Worker, endpoint};
use chigiri_mock::store::Store;
use serde_json::{Value, json};

use crate::support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(
    clippy::await_holding_lock,
    reason = "the shared integration-test guard intentionally spans the whole HTTP lifecycle"
)]
async fn tl_12_host_controller_drives_real_http_local_and_world_time() {
    let _case = support::test_case();
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "timeleap.graphql",
            "type Query { now: Int! active(id: ID!): Boolean! } \
             type Mutation { create(id: ID!, expiresAt: Int!): Item! } \
             type Item { id: ID! expiresAt: Int! }",
        )])
        .unwrap(),
    );
    let contract = Contract::new(ir.clone()).unwrap();
    let resolvers = vec![
        resolver(&contract, "Query.now", "return assert(os.time())"),
        resolver(
            &contract,
            "Query.active",
            "local rows=ctx.models.Item:find({where={field='id',op='eq',value=args.id},limit=1}) \
             if #rows == 0 then return false end \
             return rows[1].expiresAt > assert(os.time())",
        ),
        resolver(
            &contract,
            "Mutation.create",
            "return ctx.models.Item:create({id=args.id,expiresAt=args.expiresAt})",
        ),
    ];
    let hook = HookCandidate {
        slot: HookSlot::OperationBefore,
        source: render_hook_module(
            HookSlot::OperationBefore,
            r#"
if ctx.operation.name == "Local" then
    timeleap.travel_to(50)
elseif ctx.operation.name == "Reset" then
    timeleap.travel_to(50)
    timeleap.reset()
end
return nil
"#,
        )
        .unwrap(),
    };
    let hooks = contract.hooks(vec![hook], None).unwrap();
    let scripts = ScriptSet::new(contract, resolvers).unwrap();
    let mut worker = Worker::from_checked_with_clock(
        scripts,
        hooks,
        Store::empty(ir),
        chigiri_mock::clock::ClockSettings::fixed(100).unwrap(),
    )
    .unwrap();
    let control = worker.control();
    let endpoint = endpoint(worker.client.clone(), false, |_, _| {
        chigiri::FieldError::public("Resolver failed")
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = chigiri_axum::graphiql::routes(endpoint, Paths::default()).layer(
        axum::middleware::from_fn(move |request, next: axum::middleware::Next| async move {
            HttpScope::new(address).run(next.run(request)).await
        }),
    );
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopping.await;
            })
            .await
            .unwrap();
    });

    assert_eq!(integer(address, "query Normal { now }", "now").await, 100);
    let created = operation(
        address,
        "mutation { create(id: \"job\", expiresAt: 150) { id expiresAt } }",
    )
    .await;
    assert_eq!(created["data"]["create"]["id"], "job");
    assert_eq!(created["data"]["create"]["expiresAt"], 150);
    assert!(boolean(address, "query { active(id: \"job\") }", "active").await);
    control
        .run_luau("timeleap.travel_to(200, {world=true})")
        .unwrap();
    assert_eq!(integer(address, "query Normal { now }", "now").await, 200);
    assert!(!boolean(address, "query { active(id: \"job\") }", "active").await);
    assert_eq!(integer(address, "query Local { now }", "now").await, 50);
    assert_eq!(integer(address, "query Reset { now }", "now").await, 200);
    assert_eq!(integer(address, "query Normal { now }", "now").await, 200);
    control.run_luau("timeleap.reset({world=true})").unwrap();
    assert_eq!(integer(address, "query Normal { now }", "now").await, 100);
    assert!(boolean(address, "query { active(id: \"job\") }", "active").await);

    let _ = stop.send(());
    server.await.unwrap();
    worker
        .stop(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
}

fn resolver(contract: &Contract, coordinate: &str, body: &str) -> ScriptCandidate {
    let graph = contract.ir().graph();
    let (parent, field) = coordinate.split_once('.').unwrap();
    let field = graph.types()[graph.type_id(parent).unwrap().0].fields[field];
    let script = ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap(),
    );
    ScriptCandidate {
        script,
        source: render_module(contract.ir(), script, body).unwrap(),
    }
}

async fn integer(address: std::net::SocketAddr, query: &str, field: &str) -> i64 {
    operation(address, query).await["data"][field]
        .as_i64()
        .unwrap()
}

async fn boolean(address: std::net::SocketAddr, query: &str, field: &str) -> bool {
    operation(address, query).await["data"][field]
        .as_bool()
        .unwrap()
}

async fn operation(address: std::net::SocketAddr, query: &str) -> Value {
    let body = serde_json::to_string(&json!({"query": query})).unwrap();
    let response = tokio::task::spawn_blocking(move || {
        support::request(
            address,
            "POST",
            &[("Content-Type", "application/json")],
            &body,
        )
    })
    .await
    .unwrap();
    assert_eq!(response.status, 200, "{}", response.body);
    serde_json::from_str(&response.body).unwrap()
}
