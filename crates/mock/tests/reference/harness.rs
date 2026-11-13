//! Capture CLI output in real files so large stdout cannot block child processes, and enforce watchdogs.

use std::{
    fs::{self, File},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

pub const BINARY: &str = env!("CARGO_BIN_EXE_chigiri");
pub const CONFIG: &str = include_str!("../../fixtures/mock-v11-ready.toml");
pub const BLOG: &str = include_str!("../../fixtures/blog.graphql");
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Directory(pub PathBuf);

impl Directory {
    pub fn new() -> Self {
        let parent =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mock-reference-tests");
        fs::create_dir_all(&parent).unwrap();
        loop {
            let path = parent.join(format!(
                "run-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create test directory: {error}"),
            }
        }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .arg("mock")
            .current_dir(&self.0)
            .env_clear()
            .env("PATH", "")
            .stdin(Stdio::null());
        command
    }

    pub fn run(&self, arguments: &[&str]) -> Output {
        self.run_command(self.command().args(arguments))
    }

    pub fn run_command(&self, command: &mut Command) -> Output {
        self.run_command_with_timeout(command, Duration::from_secs(60))
    }

    pub fn run_command_with_timeout(&self, command: &mut Command, timeout: Duration) -> Output {
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.0.join(format!("stdout-{index}"));
        let stderr = self.0.join(format!("stderr-{index}"));
        let child = command
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let status = wait(child, timeout);
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
}

pub fn wait(mut child: Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("reference CLI watchdog expired");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
