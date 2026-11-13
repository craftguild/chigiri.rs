//! Bounded HTTP harness for the real binary; watchdog termination is never a successful stop.

use serde_json::Value;
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

mod http;
mod modules;
mod server;
pub use http::{Response, read_response, request, request_path};
pub use server::Server;
mod offline;
pub use modules::module;

pub const TEMPLATE: &str = include_str!("../../fixtures/mock-v11-ready.toml");
pub const BINARY: &str = env!("CARGO_BIN_EXE_chigiri");
static NEXT: AtomicUsize = AtomicUsize::new(0);
// Serialize full-module setup and startup while leaving request, VM, and shutdown deadlines independent.
const PREPARATION_WATCHDOG: Duration = Duration::from_secs(30);

pub fn test_case() -> std::sync::MutexGuard<'static, ()> {
    // Avoid CPU contention between unrelated test servers without restricting concurrency within one case.
    static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    CASES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mock-tests");
        fs::create_dir_all(&parent).unwrap();
        loop {
            let path = parent.join(format!(
                "run-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test directory: {error}"),
            }
        }
    }

    pub fn command(&self, arguments: &[&str]) -> Output {
        // Drain progress output independently so pipe capacity cannot block the test or its watchdog.
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.0.join(format!("command-{id}.stdout"));
        let stderr = self.0.join(format!("command-{id}.stderr"));
        let mut child = Command::new(BINARY)
            .arg("mock")
            .args(arguments)
            .current_dir(&self.0)
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + PREPARATION_WATCHDOG;
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("CLI watchdog expired");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Output {
            status: child.wait().unwrap(),
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
}

pub fn edited(root: &str, body: &str) -> String {
    // Convert the legacy return-fixture argument table to the current explicit create input.
    let body = body
        .replace(
            "ctx.models.User:create(args.input)",
            "ctx.models.User:create({id = 'User-' .. (ctx.models.User:count({}) + 1), name = args.input.name})",
        )
        .replace(
            "ctx.models.Post:create(args.input)",
            "ctx.models.Post:create({id = 'Post-' .. (ctx.models.Post:count({}) + 1), title = args.input.title, body = args.input.body, authorId = args.input.authorId})",
        )
        .replace(
            "ctx.models.Post:create({title=\"temporary\",body=\"temporary\",authorId=user.id})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"temporary\",body=\"temporary\",authorId=user.id})",
        )
        .replace(
            "ctx.models.Post:create({title=\"\",body=\"\",authorId=\"missing\"})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"\",body=\"\",authorId=\"missing\"})",
        )
        .replace(
            "ctx.models.Post:create({title=\"from script\",body=\"\",authorId=user.id})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"from script\",body=\"\",authorId=user.id})",
        )
        .replace(
            "ctx.models.Post:create({title=\"temporary\", body=\"body\", authorId=user.id})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"temporary\", body=\"body\", authorId=user.id})",
        )
        .replace(
            "ctx.models.Post:create({title=\"temporary\",body=\"temporary\",authorId=user.id})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"temporary\",body=\"temporary\",authorId=user.id})",
        )
        .replace(
            "ctx.models.Post:create({title=\"temporary\",authorId=user.id})",
            "ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title=\"temporary\",body=\"\",authorId=user.id})",
        )
        .replace(
            "ctx.models.User:create({name='nested'})",
            "ctx.models.User:create({id='User-' .. (ctx.models.User:count({}) + 1),name='nested'})",
        )
        .replace(
            "ctx.models.Post:update(args.id, patch)",
            "ctx.models.Post:update({where = {field = 'id', op = 'eq', value = args.id}}, patch)[1]",
        )
        .replace(
            "ctx.models.Post:update(\"Post-1\", {",
            "ctx.models.Post:update({where={field='id',op='eq',value=\"Post-1\"}}, {",
        )
        .replace(
            "ctx.models.Post:delete(\"Post-1\")",
            "ctx.models.Post:delete({where={field='id',op='eq',value=\"Post-1\"}})",
        );
    let body = module(&TEMPLATE.parse().unwrap(), root, &body);
    let start = TEMPLATE
        .find(&format!("[resolvers.\"{root}\"]\nluau = '''\n"))
        .unwrap();
    let body_start = start + TEMPLATE[start..].find("'''\n").unwrap() + 4;
    let end = body_start + TEMPLATE[body_start..].find("'''").unwrap();
    format!("{}{body}\n{}", &TEMPLATE[..body_start], &TEMPLATE[end..])
}

pub fn seed(config: &str) -> Value {
    let document = config.parse::<toml_edit::DocumentMut>().unwrap();
    serde_json::from_str(document["seed"]["records_json"].as_str().unwrap()).unwrap()
}

pub fn with_seed(config: &str, seed: &Value) -> String {
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    document["seed"]["records_json"] = toml_edit::value(seed.to_string());
    document.to_string()
}

pub fn without(config: &str, key: &str) -> String {
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    assert!(document.remove(key).is_some());
    document.to_string()
}

pub fn body_position(config: &str, coordinate: &str) -> (usize, usize) {
    let document = config.parse::<toml_edit::ImDocument<String>>().unwrap();
    let span = document["resolvers"][coordinate]["luau"].span().unwrap();
    let prefix = &config[..span.start];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap().chars().count() + 1,
    )
}
