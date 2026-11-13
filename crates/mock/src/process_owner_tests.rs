//! Contract tests for the shared child-tree owner through Mock policy.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::process_owner::{ChildError, OwnedChild};

#[test]
fn ai_08_child_wait_observes_exit_before_group_cleanup() {
    let mut child = OwnedChild::spawn(
        Command::new("/usr/bin/true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .unwrap();
    let deadline = Instant::now() + crate::limits::SHUTDOWN_TIME;
    while !child.exited().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(child.reap_until(deadline).unwrap().success());
}

#[test]
fn ai_08_child_stop_checks_wait_and_group_absence_before_deadline() {
    let mut child = sleeping_child();
    let pid = rustix::process::Pid::from_raw(child.process_id() as i32).unwrap();
    let start = Instant::now();
    child
        .reap_until(start + crate::limits::SHUTDOWN_TIME)
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(rustix::io::Errno::SRCH)
    );
}

#[test]
fn ai_08_cleanup_budget_owns_wait_and_group_absence() {
    let mut child = sleeping_child();
    let pid = rustix::process::Pid::from_raw(child.process_id() as i32).unwrap();
    let started = Instant::now();
    let deadline = started + crate::limits::SHUTDOWN_TIME;
    child.reap_until(deadline).unwrap();
    assert!(Instant::now() < deadline);
    drop(child);
    assert!(Instant::now() < deadline);
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(rustix::io::Errno::SRCH)
    );
}

#[test]
fn ai_08_expired_cleanup_budget_fails_closed_without_an_unbounded_wait() {
    const CHILD: &str = "CHIGIRI_EXPIRED_OWNER_CHILD";
    const PID_FILE: &str = "CHIGIRI_EXPIRED_OWNER_PID_FILE";
    if std::env::var_os(CHILD).is_some() {
        let mut child = sleeping_child();
        std::fs::write(
            std::env::var_os(PID_FILE).unwrap(),
            child.process_id().to_string(),
        )
        .unwrap();
        assert_eq!(child.reap_until(Instant::now()), Err(ChildError::Cleanup));
        drop(child);
        unreachable!("expired owner drop must fail closed");
    }

    let pid_file = std::env::temp_dir().join(format!(
        "chigiri-expired-owner-{}-{}.pid",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "process_owner_tests::ai_08_expired_cleanup_budget_fails_closed_without_an_unbounded_wait",
        ])
        .env(CHILD, "1")
        .env(PID_FILE, &pid_file)
        .stdin(Stdio::null());
    let started = Instant::now();
    let output = chigiri_host::process::run(
        &mut command,
        &[],
        chigiri_host::process::ProcessLimits {
            timeout: Duration::from_secs(2),
            bytes: 8192,
        },
        || false,
    )
    .unwrap();
    assert!(!output.status.success());
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    wait_for_reaper(pid, Instant::now() + Duration::from_secs(1));
    std::fs::remove_file(pid_file).unwrap();
}

fn wait_for_reaper(pid: rustix::process::Pid, deadline: Instant) {
    loop {
        let process = rustix::process::test_kill_process(pid);
        let group = rustix::process::test_kill_process_group(pid);
        if process == Err(rustix::io::Errno::SRCH) && group == Err(rustix::io::Errno::SRCH) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "process={process:?}, group={group:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn sleeping_child() -> OwnedChild {
    OwnedChild::spawn(
        Command::new("/bin/sleep")
            .arg("20")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .unwrap()
}
