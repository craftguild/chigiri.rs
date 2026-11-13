//! Test unchanged overrides and real doctor-process overflow recovery.

use std::fs;
use std::time::{Duration, Instant};

use super::doctor_fixture::{inputs, prepare, tool_log};
use super::fixture::report;

#[test]
fn diag_07_rejects_lockfile_updates_and_target_overrides() {
    let fixture = prepare("");
    let before = inputs(&fixture);
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    for flag in ["--allow-lockfile-update", "--all-targets"] {
        let output = fixture.execute(
            fixture
                .command("doctor")
                .arg(flag)
                .env("PATH", &paths)
                .env("CHIGIRI_TEST_CARGO_LOG", &log)
                .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(report(&output).to_string().contains("SF4001"));
        assert_eq!(inputs(&fixture), before);
        assert!(!log.exists());
    }
}

#[test]
fn diag_12_bounds_child_group_output_and_waits_for_completion() {
    let fixture = prepare("");
    let overflow_input = fixture.root.join("cargo-overflow.bin");
    // Payload creation is setup; the watchdog covers the real stream and cleanup.
    fs::write(&overflow_input, vec![b'x'; 65 * 1024 * 1024]).unwrap();
    let before = inputs(&fixture);
    let paths = fixture.wrapper("cargo", include_str!("../sibling_cli/overflow-cargo.sh"));
    let pid_file = fixture.root.join("child.pid");
    let started = Instant::now();
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_OVERFLOW_INPUT", &overflow_input)
            .env("CHIGIRI_TEST_OVERFLOW_PID", &pid_file),
    );
    let elapsed = started.elapsed();
    assert_eq!(output.status.code(), Some(2), "doctor elapsed: {elapsed:?}");
    assert!(
        report(&output).to_string().contains("SF4017"),
        "overflow diagnostic missing; doctor elapsed: {elapsed:?}"
    );
    assert_eq!(inputs(&fixture), before);
    let pid = fs::read_to_string(pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap()),
        Err(rustix::io::Errno::SRCH),
        "descendant {pid} survived; doctor elapsed: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "doctor took {elapsed:?}; SF4017, unchanged inputs and descendant cleanup verified"
    );
}
