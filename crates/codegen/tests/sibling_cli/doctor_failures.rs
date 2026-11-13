//! Test real doctor lock, runtime, tool, and cancellation failures.

use std::fs;
use std::time::{Duration, Instant};

use super::doctor_fixture::{inputs, lock, prepare, tool_log};
use super::fixture::report;

#[test]
fn sg_proc_02_missing_lockfile_rejects_update_requests_without_creating_a_lockfile() {
    let fixture = prepare("");
    let path = fixture.root.join("consumer/Cargo.lock");
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    let output = fixture.run("doctor");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        report(&output)["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["code"] == "SF4015")
    );
    assert!(!path.exists());
    fs::write(&path, &original).unwrap();
    add_dependency(&fixture, "lock-addition", "0.1.0", "lock_addition");
    let output = fixture.run("doctor");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report(&output).to_string().contains("lockfile"));
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn rejects_consumers_that_mix_core_types_from_another_source() {
    let fixture = prepare("");
    add_dependency(&fixture, "chigiri_core", "0.2.0", "other_core");
    lock(&fixture);
    let original = inputs(&fixture);
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(report(&output).to_string().contains("runtime"));
    assert_eq!(fs::read_to_string(log).unwrap(), "metadata\nmetadata\n");
    assert_eq!(inputs(&fixture), original);
}

#[test]
fn sg_cli_07_malformed_cargo_metadata_is_redacted_and_returns_exit_two() {
    let fixture = prepare("");
    let original = inputs(&fixture);
    let paths = fixture.wrapper("cargo", "#!/bin/sh\n# Simulate a failing tool that exposes secrets.\nprintf '%s' '{ password=private-password https://user:private-password@example.org /outside/private.rs'\n");
    let output = fixture.execute(fixture.command("doctor").env("PATH", paths));
    assert_eq!(output.status.code(), Some(2));
    let value = report(&output);
    assert!(value.to_string().contains("SF4017"));
    for forbidden in [
        "private-password",
        "example.org",
        "/outside",
        fixture.root.to_str().unwrap(),
    ] {
        assert!(!value.to_string().contains(forbidden));
    }
    assert_eq!(inputs(&fixture), original);
}

#[test]
fn doctor_rejects_a_cache_inside_the_sdk_before_compilation() {
    let fixture = prepare("");
    let sdk = fixture.snapshot();
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("CARGO_TARGET_DIR", fixture.output()),
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output).to_string().contains("SF4023"));
    assert_eq!(fixture.snapshot(), sdk);
}

#[test]
fn sg_cli_06_doctor_deny_warning_policy_returns_one_report() {
    let fixture = prepare("");
    let paths = fixture.wrapper("cargo", include_str!("warning-cargo.sh"));
    for (deny, status) in [(true, 1), (false, 0)] {
        let output = fixture.execute(
            fixture
                .command_with_policy("doctor", deny)
                .env("PATH", &paths)
                .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
        );
        assert_eq!(output.status.code(), Some(status));
        let value = report(&output);
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| record["code"] == "fixture_warning")
        );
        assert_eq!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| record["code"] == "SF4028"),
            deny
        );
    }
}

#[test]
fn doctor_termination_reaps_descendants_and_preserves_the_sdk_tree() {
    let fixture = prepare("");
    let original = inputs(&fixture);
    let sdk = fixture.snapshot();
    let paths = fixture.wrapper(
        "cargo",
        include_str!("../../src/contract/write/term_tests/blocking-tool.sh"),
    );
    let ready = fixture.root.join("ready");
    let observed = ready.clone();
    let trigger = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Ok(source) = fs::read_to_string(&observed) {
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
                    break pids;
                }
            }
            assert!(Instant::now() < deadline, "tool did not become ready");
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_TERM_TOOL", "cargo")
            .env("CHIGIRI_TEST_TERM_REAL_TOOL", env!("CARGO"))
            .env("CHIGIRI_TEST_TERM_READY", ready),
    );
    let pids = trigger.join().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output).to_string().contains("SF4026"));
    assert_eq!(inputs(&fixture), original);
    assert_eq!(fixture.snapshot(), sdk);
    for pid in pids {
        assert_eq!(
            rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap()),
            Err(rustix::io::Errno::SRCH)
        );
    }
}

fn add_dependency(fixture: &super::fixture::Fixture, package: &str, version: &str, alias: &str) {
    let directory = fixture.root.join(alias);
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(
        directory.join("src/lib.rs"),
        "//! doctorDependency ofidentityFor testing.fixture.\n",
    )
    .unwrap();
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname={package:?}\nversion={version:?}\nedition=\"2024\"\n[workspace]\n"
        ),
    )
    .unwrap();
    let path = fixture.root.join("consumer/Cargo.toml");
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str(&format!(
        "{alias}={{package={package:?},path=\"../{alias}\"}}\n"
    ));
    fs::write(path, source).unwrap();
}
