//! Verify scope recovery and explicit cancellation, using a child process for real signals.

use super::{Cancellation, check, recovering, requested};

#[test]
fn sg_proc_01_propagates_external_cancellation_to_nested_scopes_during_recovery() {
    let outer = Cancellation::default();
    let inner = Cancellation::default();
    assert!(!requested());
    outer.run(|| {
        assert!(!requested());
        outer.cancel();
        inner.run(|| {
            assert!(requested());
            recovering(|| assert!(!requested()));
            assert!(requested());
        });
        assert!(check().unwrap_err().to_string().contains("SF4026"));
    });
    assert!(!requested());
    inner.run(|| assert!(!requested()));
    outer.run(|| assert!(requested()));
}

#[test]
fn sg_proc_01_panicking_scope_is_removed_and_other_threads_can_recover() {
    let token = Cancellation::default();
    token.cancel();
    let panic = std::panic::catch_unwind(|| {
        token.run(|| {
            assert!(requested());
            assert!(!std::thread::spawn(requested).join().unwrap());
            recovering(|| panic!("Scope recovery test"));
        })
    });
    assert!(panic.is_err());
    assert!(!requested());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sg_fs_05_converts_term_and_int_into_isolated_process_cancellation() {
    for signal in ["term", "int"] {
        let output = crate::run_bounded_process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cancellation::tests::signal_child"])
                .env("CHIGIRI_TEST_CANCEL_SIGNAL", signal),
            &[],
            crate::ProcessLimits {
                timeout: std::time::Duration::from_secs(5),
                bytes: 4096,
            },
        )
        .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn signal_child() {
    let Ok(signal) = std::env::var("CHIGIRI_TEST_CANCEL_SIGNAL") else {
        return;
    };
    super::with_signal_cancellation(|| {
        assert!(!requested());
        let signal = match signal.as_str() {
            "term" => signal_hook::consts::SIGTERM,
            "int" => signal_hook::consts::SIGINT,
            _ => panic!("Undefined signal fixture"),
        };
        signal_hook::low_level::raise(signal).unwrap();
        assert!(requested());
        recovering(|| assert!(!requested()));
        assert!(requested());
    })
    .unwrap();
}
