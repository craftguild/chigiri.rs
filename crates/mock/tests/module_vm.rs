//! Isolate hostile module initialization in a child process and verify the next VM recovers.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source, Value};
use chigiri_mock::cancellation::Cancellation;
use chigiri_mock::ir::{MockIr, ScriptId};
use chigiri_mock::luau::{Contract, ScriptCandidate};
use chigiri_mock::output::OutputValue;
use chigiri_mock::runtime::execute_root;
use chigiri_mock::store::Store;

#[test]
fn mp_21_22_module_initialization_stops_recovers_and_joins() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "module_child", "--ignored", "--nocapture"])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("module VM watchdog expired");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "hostile module input runs only through the parent watchdog"]
fn module_child() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(boundaries)
        .unwrap()
        .join()
        .unwrap();
}

const RETURN: &str = "return {resolve=function(): number return 42 end}";

fn candidate(source: String) -> ScriptCandidate {
    ScriptCandidate {
        script: ScriptId(0),
        source,
    }
}

fn boundaries() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "schema/001.graphql",
            "type Query { value: Int! }",
        )])
        .unwrap(),
    );
    let contract = Contract::new(ir.clone()).unwrap();
    for (name, phase, initialization) in [
        ("loop", "timeout", "while true do end"),
        (
            "caught loop",
            "timeout",
            "pcall(function() while true do end end)",
        ),
        (
            "memory",
            "memory",
            "local s = 'x' for i = 1, 27 do s = s .. s end",
        ),
        (
            "caught memory",
            "memory",
            "pcall(function() local s = 'x' for i = 1, 27 do s = s .. s end end)",
        ),
        (
            "pcall17",
            "limit",
            "local function dive(n: number): () if n == 0 then return end pcall(dive, n - 1) end dive(17)",
        ),
    ] {
        let start = Instant::now();
        let error = contract
            .script(candidate(format!("{initialization}\n{RETURN}")))
            .err()
            .unwrap();
        assert_eq!(error.phase, phase, "{name}: {error}");
        assert!(start.elapsed() < Duration::from_secs(3));
        let good = contract.script(candidate(RETURN.into())).unwrap();
        let result = execute_root(
            &good,
            &Object::new(),
            &mut Store::empty(ir.clone()),
            &Cancellation::default(),
            &Cancellation::default(),
        )
        .unwrap();
        assert!(matches!(result, OutputValue::Scalar(Value::Int(42))));
        let ir = ir.clone();
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                Contract::new(ir)
                    .unwrap()
                    .script(candidate(RETURN.into()))
                    .unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
        println!("{name}: initialization rejected, next request succeeded, worker joined");
    }
    let pcall = "local function dive(n: number): () if n == 0 then return end local ok = pcall(dive, n - 1) assert(ok) end for i=1,4 do dive(16) local ok=pcall(function() error('ordinary') end) assert(not ok) end";
    contract
        .script(candidate(format!("{pcall}\n{RETURN}")))
        .unwrap();
    println!("pcall16: initialization succeeded after repeated normal failures");
}
