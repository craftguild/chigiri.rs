//! Checks help, version, and usage errors from the real example binary without external tools.

use std::process::{Command, Output};

fn run(binary: &str, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .env("PATH", "")
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap()
}

#[test]
fn dx35_01_02_03_books_help_version_and_private_usage_do_not_bind() {
    let binary = env!("CARGO_BIN_EXE_chigiri_books");
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["help"],
        vec!["--version"],
        vec!["-V"],
    ] {
        let output = run(binary, &args);
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
    for args in [
        vec!["private-token"],
        vec!["127.0.0.1:0", "extra"],
        vec!["--unknown=private-token"],
        vec!["--help", "--help"],
        vec!["--help", "--version"],
    ] {
        let output = run(binary, &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-token"));
    }
}

#[cfg(feature = "production-validation")]
#[test]
fn dx35_01_02_03_benchmark_all_help_and_usage_do_not_observe_source_or_measure() {
    let binary = env!("CARGO_BIN_EXE_production-benchmark");
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["help"],
        vec!["record", "--help"],
        vec!["compare", "-h"],
        vec!["help", "compare"],
        vec!["--version"],
        vec!["-V"],
    ] {
        let output = run(binary, &args);
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
    for args in [
        vec!["record"],
        vec!["compare", "--help", "--unknown=private-token"],
        vec!["record", "--toolchain=private-token"],
        vec!["record", "--help", "--help"],
        vec!["--help", "record", "--help"],
    ] {
        let output = run(binary, &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-token"));
    }
}
