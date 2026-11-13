//! Use an external watchdog to distinguish VM self-stop from forced termination and verify recovery.

#![forbid(unsafe_code)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chigiri_mock::project::evaluate_config;

const CONFIG: &str = r#"
return {
    hooks = {request = {before = false, after = false}, operation = {before = false, after = false}, field = {before = false, after = false}},
    state = {fields = {}, initial = {}},
    format = "chigiri-mock-project/v10",
    schema = {sources = {"schema.graphql"}},
    storage = {sources = {}},
    clock = {mode = "system"},
    server = {listen = "127.0.0.1:0", allowed_origins = {}},
}
"#;

#[test]
fn mp_05_config_vm_stops_recovers_and_releases_admission() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "config_child", "--ignored", "--nocapture"])
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
#[ignore = "hostile configuration input runs only through the parent test watchdog"]
fn config_child() {
    std::panic::set_hook(Box::new(|_| eprintln!("isolated config VM failure")));
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(boundaries)
        .unwrap()
        .join()
        .unwrap();
}

fn boundaries() {
    for (name, code) in [
        ("loop", "while true do end"),
        ("caught loop", "pcall(function() while true do end end)"),
        ("memory", "local s = 'x' for i = 1, 27 do s = s .. s end"),
        (
            "caught memory",
            "pcall(function() local s = 'x' for i = 1, 27 do s = s .. s end end)",
        ),
        (
            "pcall17",
            "local function dive(n: number): () if n == 0 then return end pcall(dive, n - 1) end dive(17)",
        ),
    ] {
        let start = Instant::now();
        let error = evaluate_config(&format!("{code}\n{CONFIG}")).err().unwrap();
        assert_eq!(error.phase, "runtime", "{name}");
        assert!(start.elapsed() < Duration::from_secs(3), "{name}");
        assert_eq!(evaluate_config(CONFIG).unwrap().sources, ["schema.graphql"]);
        // A failed VM or handle must not block a new VM on another thread.
        std::thread::spawn(|| assert!(evaluate_config(CONFIG).is_ok()))
            .join()
            .unwrap();
        println!("{name}: rejected, next VM succeeded, worker joined");
    }
    let source = format!(
        r#"
local function dive(n: number): number
    if n == 0 then return 7 end
    local ok, value = pcall(dive, n - 1)
    assert(ok) return value
end
local function failing(n: number): ()
    if n == 0 then error('expected') end
    local ok = pcall(failing, n - 1)
    if n == 1 then assert(not ok) else assert(ok) end
end
for i = 1, 4 do assert(dive(16) == 7) failing(16) end
{CONFIG}
"#
    );
    assert!(evaluate_config(&source).is_ok());
    // Exercise compiler, configuration, and runtime environment checks through the shared VM process gate.
    std::thread::scope(|scope| {
        for _ in 0..3 {
            scope.spawn(|| {
                for _ in 0..3 {
                    assert!(evaluate_config(CONFIG).is_ok());
                    assert!(
                        chigiri_mock::runtime::inspect_environment()
                            .unwrap()
                            .string_methods_restricted
                    );
                }
            });
        }
    });
}
