//! Kill a child transaction at selected points and verify markers, rollback, and rejection by later commands.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::filesystem::TRANSACTION;
use super::transaction::Point;
use super::transaction_test_support::fixture;
use super::update::update_with;

#[test]
fn mp_15_interrupted_process_leaves_marker_and_later_commands_refuse_partial_or_committed_tree() {
    let _test_case = crate::vm_gate::test_case();
    for point in ["partial", "committed"] {
        let fixture = fixture();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "project::transaction_crash_tests::transaction_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CHIGIRI_TEST_TRANSACTION_PROJECT", &fixture.project)
            .env("CHIGIRI_TEST_TRANSACTION_POINT", point)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(73));
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("transaction child watchdog expired");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            fixture
                .project
                .join(TRANSACTION)
                .join("manifest.json")
                .is_file()
        );
        assert!(fixture.project.join(TRANSACTION).join("backup-0").is_file());
        assert_eq!(
            super::read_project(&fixture.project).err().unwrap().message,
            "unfinished project transaction"
        );
        assert_eq!(
            super::update_project(&fixture.project)
                .err()
                .unwrap()
                .message,
            "unfinished project transaction"
        );
        let output = fixture.base.join("bundle.toml");
        assert!(super::export_project(&fixture.project, &output).is_err());
        assert!(!output.exists());
        // Project reference rejects the same incomplete generation as runtime readers.
        assert!(super::read_project_schema(&fixture.project).is_err());
    }
}

#[test]
#[ignore = "The intentionally terminated process is only triggered by the parent test watchdog."]
fn transaction_child() {
    let path = std::env::var_os("CHIGIRI_TEST_TRANSACTION_PROJECT").unwrap();
    let target = match std::env::var("CHIGIRI_TEST_TRANSACTION_POINT")
        .unwrap()
        .as_str()
    {
        "partial" => Point::AfterInstall(1),
        "committed" => Point::Committed,
        _ => panic!("unknown test point"),
    };
    let result = update_with(std::path::Path::new(&path), |point| {
        if point == target {
            std::process::exit(73);
        }
        Ok(())
    });
    panic!(
        "transaction exit point not reached: {}",
        result.err().unwrap()
    );
}
