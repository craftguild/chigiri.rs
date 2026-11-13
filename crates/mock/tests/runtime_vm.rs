//! Isolate hostile VM inputs in child processes and verify termination and worker recovery.

#![forbid(unsafe_code)]

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source};
use chigiri_mock::cancellation::Cancellation;
use chigiri_mock::ir::{MockIr, ScriptId};
use chigiri_mock::luau::{Contract, ScriptCandidate};
use chigiri_mock::runtime::execute_root;
use chigiri_mock::store::Store;

#[path = "runtime_vm/concurrency.rs"]
mod concurrency;
#[path = "runtime_vm/worker.rs"]
mod worker;

#[test]
fn rsm_15_17_generic_vm_stops_rolls_back_recovers_and_joins() {
    isolated("runtime_child");
}

#[test]
fn rsm_15_16_typecheck_and_vm_share_process_gate_under_concurrency() {
    isolated("concurrency_child");
}

fn isolated(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", case, "--ignored", "--nocapture"])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("watchdog termination is not successful VM self-termination");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "parallel initialization runs only through the parent test watchdog"]
fn concurrency_child() {
    concurrency::run();
}

#[test]
#[ignore = "The child process is only started by the parent test watchdog."]
fn runtime_child() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            boundaries();
            worker::cancel_running_future();
        })
        .unwrap()
        .join()
        .unwrap();
    println!("generic VM worker joined");
}

fn boundaries() {
    let source = r#"
        type Item { id: ID! name: String! }
        type Query { items: [Item!]! }
        type Mutation { run: Item! }
    "#;
    let ir = Arc::new(MockIr::parse(vec![Source::new("bounds.graphql", source)]).unwrap());
    let contract = Contract::new(ir.clone()).unwrap();
    let id = ir
        .scripts()
        .iter()
        .position(|plan| ir.graph().fields()[plan.field.0].name.as_ref() == "run")
        .unwrap();
    let build = |body: &str| {
        contract
            .script(ScriptCandidate {
                script: ScriptId(id),
                source: chigiri_mock::luau::render_module(&ir, ScriptId(id), body).unwrap(),
            })
            .unwrap()
    };
    let good = build("return ctx.models.Item:create({name = 'good'})");
    for (name, body) in [
        ("time", "while true do end"),
        ("caught time", "pcall(function() while true do end end)"),
        ("memory", "local s = 'x' for i = 1, 27 do s = s .. s end"),
        (
            "caught memory",
            "pcall(function() local s = 'x' for i = 1, 27 do s = s .. s end end)",
        ),
        (
            "pcall17",
            "local function dive(n: number): () if n == 0 then return end pcall(dive, n - 1) end dive(17)",
        ),
        (
            "host257",
            "for i = 1, 256 do ctx.models.Item:find({limit = 0}) end",
        ),
        ("json257", "for i = 1, 256 do ctx.jsonObject({}) end"),
        (
            "caught error",
            "local function failing(n: number): () if n == 0 then error('expected') end local ok = pcall(failing, n - 1) if n == 1 then assert(not ok) else assert(ok) end end failing(16)",
        ),
    ] {
        let mut store = Store::empty(ir.clone());
        let script = build(&format!(
            "local row = ctx.models.Item:create({{name = 'rolled back'}})\n{body}\nreturn row"
        ));
        let cancel = Cancellation::default();
        let shutdown = Cancellation::default();
        let start = Instant::now();
        assert!(
            execute_root(&script, &Object::new(), &mut store, &cancel, &shutdown).is_err(),
            "{name}"
        );
        assert!(start.elapsed() < Duration::from_secs(3), "{name}");
        assert_eq!(store.snapshot().size().0, 0, "{name}");
        execute_root(&good, &Object::new(), &mut store, &cancel, &shutdown).unwrap();
        assert_eq!(store.snapshot().size().0, 1);
        println!("{name}: rollback, next VM success");
    }
    // Repeat the success path 16 times; the parent case verifies captured-error cleanup and later VM recovery.
    let sixteen = build(
        r#"
        local function descend(n: number): number
            if n == 0 then return 7 end
            local ok, value = pcall(descend, n - 1)
            assert(ok) return value
        end
        for i = 1, 4 do assert(descend(16) == 7) end
        return ctx.models.Item:create({name = 'sixteen'})
    "#,
    );
    let mut store = Store::empty(ir.clone());
    execute_root(
        &sixteen,
        &Object::new(),
        &mut store,
        &Cancellation::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(store.snapshot().size().0, 1);
    let max_host = build(
        "for i = 1, 255 do ctx.models.Item:find({limit = 0}) end return ctx.models.Item:create({name = '256'})",
    );
    execute_root(
        &max_host,
        &Object::new(),
        &mut store,
        &Cancellation::default(),
        &Cancellation::default(),
    )
    .unwrap();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        execute_root(
            &good,
            &Object::new(),
            &mut store,
            &cancel,
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(store.snapshot().size().0, 2);
    let shared = Cancellation::default();
    execute_root(&good, &Object::new(), &mut store, &shared, &shared.clone()).unwrap();
    assert_eq!(store.snapshot().size().0, 3);
    // Reverse root/shutdown roles in the callback to verify one lock order across threads.
    let other = Cancellation::default();
    let deadline = Instant::now() + Duration::from_secs(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..100 {
                assert!(shared.commit(&other, deadline, || ()).is_some());
            }
        });
        scope.spawn(|| {
            for _ in 0..100 {
                assert!(other.commit(&shared, deadline, || ()).is_some());
            }
        });
    });
    // Keep the VM handle within the Rust thread boundary.
    let environment = chigiri_mock::runtime::inspect_environment().unwrap();
    assert_eq!(environment.globals.len(), 14);
    for name in ["os", "timeleap"] {
        assert!(environment.globals.iter().any(|global| global == name));
    }
    for name in [
        "io",
        "debug",
        "require",
        "load",
        "loadstring",
        "print",
        "getmetatable",
        "setmetatable",
    ] {
        assert!(!environment.globals.iter().any(|global| global == name));
    }
    assert!(environment.string_methods_restricted);
}
