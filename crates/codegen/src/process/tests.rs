//! Tests blocking, pipes, mutual waits, excess output, and descendant-process cleanup.

use std::process::Command;
use std::time::{Duration, Instant};

use super::{ProcessFailure, ProcessLimits};
use crate::run_bounded_process;

fn limits(bytes: usize) -> ProcessLimits {
    ProcessLimits {
        timeout: Duration::from_secs(3),
        bytes,
    }
}

#[test]
fn sg_proc_01_stdin_stdout_stderr_is_bounded_at_the_same_time() {
    let input = vec![b'x'; 256 * 1024];
    let output = run_bounded_process(
        Command::new("sh").args(["-c", "printf note >&2; cat"]),
        &input,
        limits(input.len() + 4),
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, input);
    assert_eq!(output.stderr, b"note");
}

#[test]
fn sg_proc_01_recovers_after_independent_stdout_and_stderr_overflow() {
    for command in [
        "while :; do printf xxxxxxxxxxxxxxxxxxxx; done",
        "while :; do printf xxxxxxxxxxxxxxxxxxxx >&2; done",
    ] {
        let start = Instant::now();
        let error =
            run_bounded_process(Command::new("sh").args(["-c", command]), &[], limits(1024))
                .unwrap_err();
        assert_eq!(error, ProcessFailure::Limit);
        assert!(start.elapsed() < Duration::from_secs(4));
    }
}

#[test]
fn proc_01_deadline_terminates_a_blocked_child_and_closes_its_pipe() {
    let start = Instant::now();
    let error = run_bounded_process(
        Command::new("sh").args(["-c", "exec 1>&- 2>&-; while :; do :; done"]),
        &[],
        ProcessLimits {
            timeout: Duration::from_millis(300),
            bytes: 1024,
        },
    )
    .unwrap_err();
    assert_eq!(error, ProcessFailure::Timeout);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn proc_01_stops_writing_large_input_when_the_child_does_not_read_stdin() {
    let input = vec![b'x'; 1024 * 1024];
    let error = run_bounded_process(
        Command::new("sh").args(["-c", "while :; do :; done"]),
        &input,
        ProcessLimits {
            timeout: Duration::from_millis(300),
            bytes: input.len(),
        },
    )
    .unwrap_err();
    assert_eq!(error, ProcessFailure::Timeout);
}

#[test]
fn sg_proc_01_parent_reaps_remaining_descendants_after_completion() {
    let output = run_bounded_process(
        Command::new("sh").args(["-c", "sleep 60 & echo $!; exit 0"]),
        &[],
        limits(1024),
    )
    .unwrap();
    assert!(output.status.success());
    let pid = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
}

#[test]
fn sg_proc_03_distinguishes_start_failures_from_pre_spawn_input_limits() {
    let mut command = Command::new("chigiri-no-such-tool-4fc53bd");
    assert_eq!(
        run_bounded_process(&mut command, &[], limits(10)).unwrap_err(),
        ProcessFailure::Start
    );
    assert_eq!(
        run_bounded_process(&mut command, &[0; 11], limits(10)).unwrap_err(),
        ProcessFailure::Limit
    );
}

#[test]
fn sg_proc_01_reports_io_failure_when_the_child_closes_stdin_early() {
    let input = vec![b'x'; 1024 * 1024];
    let error = run_bounded_process(
        Command::new("sh").args(["-c", "exec 0<&-; printf accepted; exit 0"]),
        &input,
        limits(input.len()),
    )
    .unwrap_err();
    assert_eq!(error, ProcessFailure::Io);
}
