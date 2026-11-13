//! Verifies CLI signal propagation to formatter and Cargo descendant cleanup and rollback.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::fixture::{Fixture, report};

#[test]
fn sg_fs_05_cli_termination_during_format_or_cargo_preserves_old_bytes_and_exits_two() {
    for tool in ["rustup", "cargo"] {
        let fixture = Fixture::new();
        fixture.success("codegen");
        let initial = fixture.snapshot();
        fs::write(
            fixture.root.join("graphql/schema.graphql"),
            "type Query { value: String extra: Int }",
        )
        .unwrap();
        let path = fixture.wrapper(
            tool,
            include_str!("../../src/contract/write/term_tests/blocking-tool.sh"),
        );
        let ready = fixture.root.join("ready");
        let observed = ready.clone();
        let trigger = std::thread::spawn(move || terminate_when_ready(&observed));
        let output = fixture.execute(
            fixture
                .command("update")
                .env("PATH", path)
                .env("CHIGIRI_TEST_TERM_TOOL", tool)
                .env("CHIGIRI_TEST_TERM_REAL_TOOL", find(tool))
                .env("CHIGIRI_TEST_TERM_READY", ready)
                .env(
                    "CHIGIRI_TEST_TERM_STAGE",
                    fixture.root.join("generated/.chigiri-transaction"),
                ),
        );
        let pids = trigger.join().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value = report(&output);
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| record["code"] == "SF4026")
        );
        assert_eq!(fixture.snapshot(), initial);
        assert!(!fixture.root.join("generated/.chigiri-transaction").exists());
        for pid in pids {
            assert_eq!(
                rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap()),
                Err(rustix::io::Errno::SRCH)
            );
        }
    }
}

fn find(tool: &str) -> PathBuf {
    if tool == "cargo" {
        return PathBuf::from(env!("CARGO"));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join(tool))
        .find(|path| path.is_file())
        .unwrap()
}

fn terminate_when_ready(path: &Path) -> Vec<i32> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Ok(source) = fs::read_to_string(path) {
            let pids = source
                .split_whitespace()
                .filter_map(|part| part.parse::<i32>().ok())
                .collect::<Vec<_>>();
            if pids.len() == 3 {
                rustix::process::kill_process(
                    rustix::process::Pid::from_raw(pids[0]).unwrap(),
                    rustix::process::Signal::TERM,
                )
                .unwrap();
                return pids;
            }
        }
        assert!(Instant::now() < deadline, "tool did not become ready");
        std::thread::sleep(Duration::from_millis(2));
    }
}
