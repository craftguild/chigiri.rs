//! Sends TERM externally to tool descendants during formatter or Cargo execution.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::contract::check::fixture::Fixture;
use crate::{ResolvedManifest, render_contract, update_contract};

use super::super::tests::{configure_cache, live};
use super::super::transaction::STAGE;

#[test]
fn sg_proc_01_terminates_formatter_and_cargo_descendants_and_preserves_the_live_tree() {
    for tool in ["rustup", "cargo"] {
        let fixture = Fixture::new();
        configure_cache(&fixture);
        fixture.write(&render_contract(&fixture.manifest).unwrap());
        let old = live(&fixture);
        fs::write(
            fixture.root.join("graphql/schema.graphql"),
            "type Query { extra: Int value: String }",
        )
        .unwrap();
        let bin = fixture.root.join("bin");
        fs::create_dir(&bin).unwrap();
        let wrapper = bin.join(tool);
        fs::write(&wrapper, include_str!("blocking-tool.sh")).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
        let ready = fixture.root.join("ready");
        let observed = ready.clone();
        let trigger = std::thread::spawn(move || terminate_when_ready(&observed));
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let output = crate::run_bounded_process(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "contract::write::term_tests::process::tool_child",
                ])
                .env("CHIGIRI_TEST_TERM_ROOT", &fixture.root)
                .env("CHIGIRI_TEST_TERM_TOOL", tool)
                .env("CHIGIRI_TEST_TERM_REAL_TOOL", find(tool))
                .env("CHIGIRI_TEST_TERM_READY", &ready)
                .env(
                    "CHIGIRI_TEST_TERM_STAGE",
                    fixture.output().parent().unwrap().join(STAGE),
                )
                .env("PATH", std::env::join_paths(paths).unwrap()),
            &[],
            crate::ProcessLimits {
                timeout: Duration::from_secs(30),
                bytes: 16 * 1024,
            },
        )
        .unwrap();
        let pids = trigger.join().unwrap();
        assert!(
            output.status.success(),
            "{tool}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(live(&fixture), old);
        assert!(!fixture.output().parent().unwrap().join(STAGE).exists());
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
    let deadline = Instant::now() + Duration::from_secs(20);
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

#[test]
fn tool_child() {
    let Some(root) = std::env::var_os("CHIGIRI_TEST_TERM_ROOT") else {
        return;
    };
    let manifest = ResolvedManifest::load(&Path::new(&root).join("chigiri.toml")).unwrap();
    let error = crate::with_signal_cancellation(|| {
        update_contract(&manifest, crate::DiagnosticPolicy::default())
    })
    .unwrap()
    .unwrap_err();
    assert!(
        error
            .report()
            .diagnostics()
            .iter()
            .any(|item| item.code() == "SF4026"),
        "{error}"
    );
}
