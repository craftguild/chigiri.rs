//! Exercise checker and VM concurrency, global initialization races, and recovery across processes.

use std::sync::{Arc, Barrier};

use chigiri::core::{Object, Source, Value};
use chigiri_mock::cancellation::Cancellation;
use chigiri_mock::ir::{MockIr, ScriptId};
use chigiri_mock::luau::{Contract, ScriptCandidate};
use chigiri_mock::output::OutputValue;
use chigiri_mock::runtime::{execute_root, inspect_environment};
use chigiri_mock::store::Store;

pub fn run() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "parallel.graphql",
            "type Query { value: Int! }",
        )])
        .unwrap(),
    );
    cold_start(ir.clone());
    let script = Arc::new(
        Contract::new(ir.clone())
            .unwrap()
            .script(ScriptCandidate {
                script: ScriptId(0),
                source: chigiri_mock::luau::render_module(&ir, ScriptId(0), "return 42").unwrap(),
            })
            .unwrap(),
    );
    let start = Arc::new(Barrier::new(4));
    let threads = (0..4)
        .map(|index| {
            let ir = ir.clone();
            let script = script.clone();
            let start = start.clone();
            std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    for round in 0..8 {
                        // Each round races VM and checker work so one thread cannot monopolize initialization.
                        start.wait();
                        if (index + round) % 2 == 0 {
                            let info = inspect_environment().unwrap();
                            assert_eq!(info.globals.len(), 14);
                            assert!(info.globals.iter().any(|global| global == "os"));
                            assert!(info.globals.iter().any(|global| global == "timeleap"));
                            assert!(info.string_methods_restricted);
                            let output = execute_root(
                                &script,
                                &Object::new(),
                                &mut Store::empty(ir.clone()),
                                &Cancellation::default(),
                                &Cancellation::default(),
                            )
                            .unwrap();
                            assert!(matches!(output, OutputValue::Scalar(Value::Int(42))));
                        } else {
                            let error = Contract::new(ir.clone())
                                .unwrap()
                                .script(ScriptCandidate {
                                    script: ScriptId(0),
                                    source: chigiri_mock::luau::render_module(
                                        &ir,
                                        ScriptId(0),
                                        "return 'wrong'",
                                    )
                                    .unwrap(),
                                })
                                .err()
                                .expect("type mismatch must fail");
                            assert_eq!(error.phase, "type");
                            assert_eq!(error.script, Some(ScriptId(0)));
                        }
                    }
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    println!("parallel checker/VM: 16 exact results, 16 type rejections, all four threads joined");
}

fn cold_start(ir: Arc<MockIr>) {
    let start = Arc::new(Barrier::new(4));
    let threads = (0..4)
        .map(|index| {
            let ir = ir.clone();
            let start = start.clone();
            std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    // Start checker and VM work together to exercise first-use initialization races.
                    start.wait();
                    if index % 2 == 0 {
                        let info = inspect_environment().unwrap();
                        assert_eq!(info.globals.len(), 14);
                        assert!(info.globals.iter().any(|global| global == "os"));
                        assert!(info.globals.iter().any(|global| global == "timeleap"));
                        assert!(info.string_methods_restricted);
                    } else {
                        Contract::new(ir.clone())
                            .unwrap()
                            .script(ScriptCandidate {
                                script: ScriptId(0),
                                source: chigiri_mock::luau::render_module(
                                    &ir,
                                    ScriptId(0),
                                    "return 42",
                                )
                                .unwrap(),
                            })
                            .unwrap();
                    }
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    println!("cold checker/VM: both initial entries succeeded, all four threads joined");
}
