//! Exercise VM behavior in a child process guarded by an external watchdog.

#![forbid(unsafe_code)]

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use luaur::{Lua, LuaOptions, StdLib, VmState};

const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

fn isolated(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "vm_child", "--ignored", "--nocapture"])
        .env("CHIGIRI_MOCK_SPIKE_CASE", case)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "VM process failure: {case}: {status}");
            return;
        }
        if started.elapsed() >= Duration::from_secs(8) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("watchdog made VM the process offspring of kill: {case}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn mock_01_expected_types() {
    isolated("types");
}

#[test]
fn mock_01_rust_host_call() {
    isolated("host");
}

#[test]
fn mock_12_interrupt_recovery_and_worker_join() {
    isolated("interrupt");
}

#[test]
fn mock_12_memory_limit_and_recovery() {
    isolated("memory");
}

#[test]
fn mock_15_actual_globals() {
    isolated("globals");
}

#[test]
#[ignore = "The parent test explicitly starts the child process."]
fn vm_child() {
    let case = std::env::var("CHIGIRI_MOCK_SPIKE_CASE").unwrap();
    // Keep Lua on its dedicated worker while exercising failure and rollback.
    std::thread::Builder::new()
        .name("mock-vm-spike".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || match case.as_str() {
            "types" => expected_types(),
            "host" => host_call(),
            "interrupt" => interrupt(),
            "memory" => memory(),
            "globals" => globals(),
            _ => panic!("unknown spike mode"),
        })
        .unwrap()
        .join()
        .unwrap();
    println!("worker joined");
}

fn wrapper(body: &str) -> String {
    format!(
        "--!strict\n\
         type User = {{ id: string, name: string }}\n\
         type Args = {{ limit: number }}\n\
         local function resolve(args: Args): {{User}}\n{body}\nend\n\
         return resolve\n"
    )
}

fn expected_types() {
    let definitions = "declare function findUsers(limit: number): {{ id: string, name: string }}";
    let good = wrapper("return findUsers(args.limit)");
    luaur::check_with_definitions(&good, definitions).unwrap();
    assert!(!luaur::compile(&good).unwrap().is_empty());
    let output = luaur::check_with_definitions(&wrapper("return 42"), definitions).unwrap_err();
    assert!(output.iter().any(|d| !d.in_definitions && d.line == 5));
    let input = luaur::check_with_definitions(&wrapper("return findUsers(\"wrong\")"), definitions)
        .unwrap_err();
    assert!(input.iter().any(|d| !d.in_definitions && d.line == 5));
    println!(
        "Typed wrapper: Normal success, return value and arguments inconsistency rejected in the first line of the main."
    );
}

fn host_call() {
    let lua = Lua::new_empty();
    let host = lua
        .create_function(|lua, limit: i32| {
            assert_eq!(limit, 20);
            let user = lua.create_table_result()?;
            user.set("id", "user-1")?;
            user.set("name", "Alice")?;
            lua.create_sequence_from([user])
        })
        .unwrap();
    lua.globals().set("findUsers", host).unwrap();
    let resolve: luaur::Function = lua
        .load(wrapper("return findUsers(args.limit)"))
        .eval()
        .unwrap();
    let args = lua.create_table_result().unwrap();
    args.set("limit", 20).unwrap();
    let users: luaur::Table = resolve.call(args).unwrap();
    let user: luaur::Table = users.get(1).unwrap();
    assert_eq!(user.get::<String>("id").unwrap(), "user-1");
    assert_eq!(user.get::<String>("name").unwrap(), "Alice");
    println!("Rust host call: Get the exact user record");
}

fn interrupt() {
    let lua = Lua::new_empty();
    lua.set_memory_limit(MEMORY_LIMIT).unwrap();
    let started = Instant::now();
    let calls = Arc::new(AtomicUsize::new(0));
    let expired = Arc::new(AtomicBool::new(false));
    let interrupt_calls = calls.clone();
    let interrupt_expired = expired.clone();
    lua.set_interrupt(move |_| {
        interrupt_calls.fetch_add(1, Ordering::Relaxed);
        if started.elapsed() >= Duration::from_millis(500) {
            interrupt_expired.store(true, Ordering::Relaxed);
            Err(luaur::Error::runtime("mock budget exceeded"))
        } else {
            Ok(VmState::Continue)
        }
    });
    let result = lua.load("while true do end").exec();
    assert!(
        matches!(result, Err(luaur::Error::RuntimeError(ref message)) if message == "mock budget exceeded")
    );
    assert!(calls.load(Ordering::Relaxed) > 0);
    assert!(expired.load(Ordering::Relaxed));
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert!(started.elapsed() < Duration::from_secs(2));
    let elapsed = started.elapsed();
    drop(lua);
    let next = Lua::new_empty();
    next.set_memory_limit(MEMORY_LIMIT).unwrap();
    assert_eq!(next.load("return 6 * 7").eval::<i32>().unwrap(), 42);
    println!("VM interrupt: {elapsed:?}, the next VM in the same worker returned 42.");
}

fn memory() {
    let lua = Lua::new_empty();
    lua.set_memory_limit(MEMORY_LIMIT).unwrap();
    let result = lua
        .load("local s = 'x' for i = 1, 27 do s = s .. s end return #s")
        .eval::<i32>();
    assert!(matches!(result, Err(luaur::Error::MemoryError(_))));
    drop(lua);
    let next = Lua::new_empty();
    next.set_memory_limit(MEMORY_LIMIT).unwrap();
    assert_eq!(next.load("return 42").eval::<i32>().unwrap(), 42);
    println!("the next VM in the same worker returned 42");
}

fn globals() {
    let selected = Lua::new_with(StdLib::TABLE, LuaOptions::default()).unwrap();
    // luaur 0.1.8 does not expose per-library labels; record observed behavior without claiming a security boundary.
    assert!(matches!(
        selected.globals().get::<luaur::Value>("os").unwrap(),
        luaur::Value::Table(_)
    ));
    let empty = Lua::new_empty();
    for name in [
        "os",
        "io",
        "debug",
        "require",
        "load",
        "loadstring",
        "package",
        "print",
    ] {
        assert!(matches!(
            empty.globals().get::<luaur::Value>(name).unwrap(),
            luaur::Value::Nil
        ));
    }
    assert_eq!(
        empty
            .globals()
            .pairs::<luaur::Value, luaur::Value>()
            .count(),
        0
    );
    println!("StdLib::TABLE will also release os.");
}
