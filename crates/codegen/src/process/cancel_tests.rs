//! Test recovery of real process trees with blocked pipes and cancelled spawns.

use std::process::Command;
use std::time::{Duration, Instant};

use super::{ProcessFailure, ProcessLimits};
use crate::Cancellation;
use crate::filesystem::tests::Fixture;
use crate::run_bounded_process;

#[test]
fn sg_proc_01_already_cancelled_token_prevents_tool_spawn() {
    let token = Cancellation::default();
    token.cancel();
    let result = token.run(|| {
        run_bounded_process(
            &mut Command::new("chigiri-no-such-tool"),
            &[],
            ProcessLimits {
                timeout: Duration::from_secs(3),
                bytes: 1024,
            },
        )
    });
    assert_eq!(result.unwrap_err(), ProcessFailure::Cancelled);
}

#[test]
fn sg_proc_01_reaps_parent_and_descendant_processes_after_cancelled_pipe_closure() {
    let fixture = Fixture::new();
    let pid_path = fixture.0.join("pids");
    let observed = pid_path.clone();
    let token = Cancellation::default();
    let trigger = token.clone();
    let watcher = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(source) = std::fs::read_to_string(&observed) {
                let pids = source
                    .split_whitespace()
                    .map(|part| part.parse::<i32>().unwrap())
                    .collect::<Vec<_>>();
                if pids.len() == 2 {
                    trigger.cancel();
                    return pids;
                }
            }
            assert!(
                Instant::now() < deadline,
                "child readiness notification is missing"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let start = Instant::now();
    let result = token.run(|| run_bounded_process(Command::new("sh")
        .args(["-c", "sleep 60 & descendant=$!; printf '%s %s' \"$$\" \"$descendant\" > \"$1\"; exec 1>&- 2>&-; wait", "chigiri-cancel-fixture"])
        .arg(&pid_path), &vec![0; 1024 * 1024], ProcessLimits {
        timeout: Duration::from_secs(5), bytes: 1024 * 1024,
    }));
    let pids = watcher.join().unwrap();
    assert_eq!(result.unwrap_err(), ProcessFailure::Cancelled);
    assert!(start.elapsed() < Duration::from_secs(5));
    for pid in pids {
        assert_eq!(
            rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap()),
            Err(rustix::io::Errno::SRCH)
        );
    }
}
