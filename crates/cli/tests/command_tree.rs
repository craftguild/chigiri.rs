//! Verify the closed unified namespace tree without project or tool access.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-command-tree-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_chigiri"))
            .args(args)
            .current_dir(&self.0)
            .env_clear()
            .env("PATH", "")
            .output()
            .unwrap()
    }

    fn run_with_closed_stdout(&self, args: &[&str]) -> Output {
        self.run_with_closed_stdout_after_pipe(args, || {})
    }

    fn run_with_closed_stdout_after_pipe(
        &self,
        args: &[&str],
        after_pipe: impl FnOnce(),
    ) -> Output {
        // Prevent parallel test children from inheriting a reader before drop.
        let (reader, writer) = std::io::pipe().unwrap();
        after_pipe();
        drop(reader);
        Command::new(env!("CARGO_BIN_EXE_chigiri"))
            .args(args)
            .current_dir(&self.0)
            .env_clear()
            .env("PATH", "")
            .stdout(Stdio::from(writer))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
            .wait_with_output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ch_01_bare_root_and_namespaces_show_offline_help_without_writes() {
    let fixture = Fixture::new();
    for (args, usage) in [
        (&[][..], "Usage: chigiri [COMMAND]"),
        (&["rust"][..], "Usage: chigiri rust"),
        (&["mock"][..], "Usage: chigiri mock"),
    ] {
        let output = fixture.run(args);
        assert!(output.status.success(), "{args:?}");
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains(usage));
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_only_root_help_includes_the_shared_banner() {
    let fixture = Fixture::new();
    let banner = include_str!("../assets/version-banner.txt");
    let bare = fixture.run(&[]);
    assert!(bare.status.success());
    assert!(bare.stderr.is_empty());
    let root = String::from_utf8(bare.stdout).unwrap();
    let mut prefix = Vec::new();
    craftguild_cli::help(
        &mut prefix,
        craftguild_cli::Brand {
            banner,
            project: "chigiri.rs",
        },
        "chigiri.rs schema-first",
    )
    .unwrap();
    assert!(root.starts_with(std::str::from_utf8(&prefix).unwrap()));
    assert_eq!(root.matches(banner).count(), 1);
    for args in [&["--help"][..], &["-h"][..], &["help"][..]] {
        let output = fixture.run(args);
        assert!(output.status.success(), "{args:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), root);
    }
    for (namespace, commands) in [
        (
            "rust",
            &["init", "codegen", "update", "check", "doctor"][..],
        ),
        (
            "mock",
            &[
                "init",
                "update",
                "codegen",
                "check",
                "serve",
                "export",
                "reference",
            ][..],
        ),
    ] {
        let mut paths = vec![vec![namespace], vec!["help", namespace]];
        for flag in ["--help", "-h", "help"] {
            paths.push(vec![namespace, flag]);
        }
        for command in commands {
            paths.push(vec![namespace, command, "--help"]);
            paths.push(vec![namespace, command, "-h"]);
            paths.push(vec![namespace, "help", command]);
            paths.push(vec!["help", namespace, command]);
        }
        for args in paths {
            let output = fixture.run(&args);
            assert!(output.status.success(), "{args:?}");
            assert!(output.stderr.is_empty());
            let help = String::from_utf8(output.stdout).unwrap();
            assert!(help.contains(&format!("Usage: chigiri {namespace}")));
            assert!(!help.contains(banner), "{args:?}");
        }
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_versions_share_banner_and_build_fields_and_remain_namespace_specific_offline() {
    let fixture = Fixture::new();
    let info = chigiri_codegen::generator_info();
    let banner = include_str!("../assets/version-banner.txt");
    assert!(banner.is_ascii());
    assert!(banner.ends_with('\n'));
    assert!(banner.lines().all(|line| !line.ends_with(' ')));
    let actual = chigiri_cli::BUILD_INFO;
    assert_eq!(actual.version, info.version.as_ref());
    assert_eq!(
        actual.commit,
        info.git_commit.as_deref().unwrap_or("unknown")
    );
    assert_eq!(actual.dirty, info.dirty);
    assert_eq!(actual.compiler, info.build.rustc.as_ref());
    assert_eq!(actual.target, info.build.target.as_ref());
    assert_eq!(actual.profile, info.build.profile.as_ref());
    let expected = |command, mock| {
        let mut body = Vec::new();
        craftguild_cli::version(
            &mut body,
            craftguild_cli::Brand {
                banner,
                project: "chigiri.rs",
            },
            craftguild_cli::BuildInfo { command, ..actual },
        )
        .unwrap();
        let mut body = String::from_utf8(body).unwrap();
        body.push_str(&format!("host: {}\n", info.build.host));
        if mock {
            body.push_str("luaur: 0.1.8\n");
        }
        body
    };
    for flag in ["--version", "-V"] {
        for (args, expected) in [
            (vec![flag], expected("chigiri", false)),
            (vec!["rust", flag], expected("chigiri rust", false)),
            (vec!["mock", flag], expected("chigiri mock", true)),
        ] {
            let output = fixture.run(&args);
            assert!(output.status.success(), "{args:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        }
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_common_terminal_paths_report_closed_stdout_as_failure() {
    let fixture = Fixture::new();
    for args in [
        &[][..],
        &["--version"][..],
        &["sbom"][..],
        &["mock", "--version"][..],
        &["mock", "--help"][..],
        &["mock", "init", "--help"][..],
        &["help", "mock", "init"][..],
    ] {
        let output = fixture.run_with_closed_stdout(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_closed_stdout_is_not_kept_open_by_an_unrelated_child() {
    let fixture = Fixture::new();
    let mut unrelated = None;
    let output = fixture.run_with_closed_stdout_after_pipe(&[], || {
        // Force another spawn before the reader closes instead of relying on
        // parallel-test timing. The owned writer waits for stdin until dropped.
        let writer = chigiri_mock::stdio_writer::Pipe::start(
            std::path::Path::new(env!("CARGO_BIN_EXE_chigiri")),
            Ok(Stdio::null()),
        );
        assert!(writer.process_id().is_some(), "unrelated child must start");
        unrelated = Some(writer);
    });
    drop(unrelated);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn sbom_is_exact_embedded_json_without_configuration_or_tools() {
    let fixture = Fixture::new();
    let output = fixture.run(&["sbom"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, chigiri_cli::SBOM);
    let bom: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bom["bomFormat"], "CycloneDX");
    assert_eq!(bom["specVersion"], "1.5");
    assert_eq!(bom["metadata"]["component"]["name"], "chigiri");
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_retired_commands_aliases_and_hidden_terminal_flags_are_usage_errors() {
    let fixture = Fixture::new();
    for args in [
        vec!["rust", "generate"],
        vec!["rust", "gen"],
        vec!["rust", "snapshot"],
        vec!["rust", "diff"],
        vec!["rust", "scaffold", "projection", "--object", "Book"],
        vec!["mock", "infer"],
        vec!["mock", "apply"],
        vec!["pythia"],
        vec!["cargo-pythia"],
        vec!["pythia-mock"],
        vec![chigiri_mock::stdio_writer::COMMAND, "--help"],
        vec![chigiri_mock::stdio_writer::COMMAND, "--version"],
        vec![chigiri_mock::stdio_writer::COMMAND, "value"],
        vec![chigiri_mock::stdio_writer::COMMAND, "--config=x"],
        vec!["__pythia-stdio-writer-v1"],
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn ch_01_mock_help_and_closed_options_use_the_product_tree() {
    let fixture = Fixture::new();
    for name in [
        "init",
        "update",
        "codegen",
        "check",
        "serve",
        "export",
        "reference",
    ] {
        for args in [
            vec!["mock", name, "--help"],
            vec!["mock", name, "-h"],
            vec!["mock", "help", name],
        ] {
            let output = fixture.run(&args);
            assert!(output.status.success(), "{args:?}");
            let help = String::from_utf8_lossy(&output.stdout);
            assert!(help.contains(name), "{help}");
            assert!(!help.contains(chigiri_mock::stdio_writer::COMMAND));
        }
    }
    let reference = fixture.run(&["mock", "reference", "--help"]);
    assert!(
        String::from_utf8_lossy(&reference.stdout)
            .contains("Names are accepted for directives, methods, hooks")
    );
    let serve = fixture.run(&["mock", "serve", "--enable-graphiql", "--help"]);
    let help = String::from_utf8_lossy(&serve.stdout);
    assert!(help.contains("schema introspection"));
    assert!(help.contains("default: off"));

    for args in [
        vec!["mock", "check", "--config=x", "--config-dir=y"],
        vec!["mock", "check", "--config=x", "--config=y"],
        vec!["mock", "init", "--schema=a"],
        vec!["mock", "init", "--config-dir=x"],
        vec!["mock", "codegen"],
        vec!["mock", "update", "--config-dir=x", "--format=json"],
        vec!["mock", "reference", "--format=TOKEN_PRIVATE"],
        vec!["mock", "reference", "--unknown=/private/secret"],
        vec!["mock", "serve", "--config"],
        vec!["mock", "reference", "--for=json"],
        vec!["mock", "check", "--config=x", "--target=resolver.luau"],
        vec!["mock", "check", "--config-dir=x", "--format=yaml"],
        vec!["mock", "serve", "--config=x", "--enable-graphiql=true"],
        vec![
            "mock",
            "serve",
            "--config=x",
            "--enable-graphiql",
            "--enable-graphiql",
        ],
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(!diagnostic.contains("TOKEN_PRIVATE"));
        assert!(!diagnostic.contains("/private/secret"));
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}
