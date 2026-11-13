//! Distinguishes reported signal results from actual surviving process state.

use super::*;

fn owner(script: &str, timeout: Duration) -> Owner {
    let child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    Owner {
        group: Pid::from_child(&child),
        child,
        deadline: Instant::now() + timeout,
        cleaned: false,
        status: None,
    }
}

#[test]
fn dx35_05_waits_for_the_process_leader_before_signaling() {
    let mut child = owner("exit 0", Duration::from_secs(3));
    while !child.exited().unwrap() {
        assert!(Instant::now() < child.deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(child.status.is_none());
    child
        .cleanup_with_signal(|pid| {
            // The real OS may report that the PID was reaped immediately before signaling.
            assert!(
                waitid(
                    WaitId::Pid(pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                )
                .unwrap()
                .is_some()
            );
            signal_group(pid)
        })
        .unwrap();
    assert!(child.status.unwrap().success());
    assert!(child.cleaned);
}

#[test]
fn reaped_process_ids_are_not_signaled_again() {
    let mut child = owner("exit 0", Duration::from_secs(3));
    child.cleanup().unwrap();
    assert!(child.status.is_some());
    // The path that requires group re-confirmation will not return the previously reaped PID to the suspended object.
    child.cleaned = false;
    child
        .cleanup_with_signal(|_| panic!("a reaped process ID must not be signaled again"))
        .unwrap();
    assert!(child.cleaned);
}

#[test]
fn sg_proc_01_signal_failure_confirms_completed_child_and_group_are_reaped() {
    let mut child = owner("exit 0", Duration::from_secs(3));
    child
        .cleanup_with_signal(|_| Err(ProcessFailure::Cleanup))
        .unwrap();
    assert!(child.cleaned);
    assert!(child.child.try_wait().unwrap().is_some());
    assert_eq!(test_kill_process_group(child.group), Err(Errno::SRCH));
}

#[test]
fn sg_proc_01_signal_failure_does_not_claim_success_while_process_survives() {
    let mut child = owner("while :; do :; done", Duration::from_millis(100));
    assert_eq!(
        child.cleanup_with_signal(|_| Err(ProcessFailure::Cleanup)),
        Err(ProcessFailure::Cleanup)
    );
    assert!(!child.cleaned);
    assert!(child.child.try_wait().unwrap().is_none());
    // Clean up the real child so a failing assertion cannot leave it running.
    child.deadline = Instant::now() + Duration::from_secs(3);
    child.cleanup().unwrap();
    assert_eq!(test_kill_process_group(child.group), Err(Errno::SRCH));
}

#[test]
fn sg_proc_01_cleanup_failure_fails_closed_by_aborting() {
    const CHILD: &str = "CHIGIRI_HOST_CLEANUP_ABORT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        require_cleanup(Err(ProcessFailure::Cleanup));
        panic!("cleanup failure returned after the fail-closed boundary");
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::unix::tests::sg_proc_01_cleanup_failure_fails_closed_by_aborting",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
}
