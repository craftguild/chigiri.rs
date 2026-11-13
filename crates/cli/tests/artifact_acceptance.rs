//! Product acceptance of verified shipped bytes; run explicitly with make acceptance.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-artifact-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(PartialEq, Eq)]
enum Entry {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, current: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let value = if kind.is_symlink() {
                Entry::Link(fs::read_link(&path).unwrap())
            } else if kind.is_dir() {
                visit(root, &path, entries);
                Entry::Directory
            } else {
                Entry::File(fs::read(&path).unwrap())
            };
            entries.insert(path.strip_prefix(root).unwrap().to_owned(), value);
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn command(binary: &Path, runtime: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .current_dir(runtime)
        .env_clear()
        .env("PATH", "")
        .env("HOME", runtime.join("home"))
        .env("GIT_DIR", runtime.join("no-git"))
        .env("HOST", "unrelated-host")
        .stdin(Stdio::null());
    command
}

fn run(binary: &Path, runtime: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = command(binary, runtime, arguments).output().unwrap();
    succeeded(&output, arguments);
    assert!(
        output.stderr.is_empty(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn succeeded(output: &Output, arguments: &[&str]) {
    assert!(
        output.status.success(),
        "{arguments:?}: status={}, {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn variable(name: &str, value: impl AsRef<OsStr>) -> OsString {
    let mut argument = OsString::from(name);
    argument.push("=");
    argument.push(value);
    argument
}

#[test]
#[ignore = "make acceptance supplies the verified engine-built artifact and selected profile"]
fn copied_cli_preserves_offline_metadata_output_failures_and_installation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let path = std::env::var_os("CHIGIRI_ACCEPTANCE_RECEIPT").expect("Run make acceptance");
    let receipt: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let context = &receipt["context"];
    let prepared = &receipt["prepared"];
    let build = &prepared["build"];
    let binary = Path::new(context["output"].as_str().unwrap());
    let expected_sbom = fs::read(root.join(prepared["sbom"].as_str().unwrap())).unwrap();
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::create_dir(runtime.join("home")).unwrap();
    let copied = runtime.join("chigiri");
    fs::copy(binary, &copied).unwrap();
    fs::write(runtime.join("craftguild.toml"), "invalid configuration [").unwrap();
    fs::write(
        runtime.join("chigiri.toml"),
        "invalid product configuration [",
    )
    .unwrap();
    let before = snapshot(&runtime);
    let banner = fs::read(root.join("crates/cli/assets/version-banner.txt")).unwrap();
    let help = run(&copied, &runtime, &[]);
    assert_eq!(help, run(&copied, &runtime, &["-h"]));
    assert_eq!(help, run(&copied, &runtime, &["--help"]));
    assert!(help.starts_with(&banner));
    let text = std::str::from_utf8(&help).unwrap();
    assert!(text.is_ascii());
    let affiliation = "craftguild/chigiri.rs";
    assert_eq!(text.matches(affiliation).count(), 1);
    let row = text
        .lines()
        .find(|line| line.trim() == affiliation)
        .unwrap();
    let width = std::str::from_utf8(&banner)
        .unwrap()
        .lines()
        .map(str::len)
        .max()
        .unwrap();
    assert_eq!(
        row,
        format!(
            "{}{}",
            " ".repeat(width.saturating_sub(affiliation.len()) / 2),
            affiliation
        )
    );

    let version = run(&copied, &runtime, &["-V"]);
    assert_eq!(version, run(&copied, &runtime, &["--version"]));
    assert!(version.starts_with(&banner));
    let human = std::str::from_utf8(&version).unwrap();
    assert!(human.contains(&format!("chigiri {}", build["version"].as_str().unwrap())));
    for field in ["commit", "compiler", "target", "profile"] {
        assert!(
            human.contains(build[field].as_str().unwrap()),
            "Version lacks {field}"
        );
    }
    assert_eq!(human.contains(" (dirty)"), build["dirty"] == "dirty");
    assert!(!help.contains(&0x1b) && !version.contains(&0x1b));
    assert!(
        run(&copied, &runtime, &["sbom"]) == expected_sbom,
        "Embedded SBOM bytes differ"
    );
    let sbom: Value = serde_json::from_slice(&expected_sbom).unwrap();
    assert_eq!(sbom["bomFormat"], "CycloneDX");
    assert_eq!(sbom["specVersion"], "1.5");
    let properties = sbom["metadata"]["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap(), p["value"].as_str().unwrap()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        serde_json::from_str::<Value>(properties["craftguild:build"]).unwrap(),
        *build
    );
    for namespace in ["rust", "mock"] {
        let help = run(&copied, &runtime, &[namespace, "--help"]);
        assert!(
            !std::str::from_utf8(&help)
                .unwrap()
                .contains(std::str::from_utf8(&banner).unwrap())
        );
        let version = run(&copied, &runtime, &[namespace, "--version"]);
        assert!(
            std::str::from_utf8(&version)
                .unwrap()
                .contains(build["version"].as_str().unwrap())
        );
    }
    let machine: Value = serde_json::from_slice(&run(
        &copied,
        &runtime,
        &["rust", "--version", "--format", "json"],
    ))
    .unwrap();
    let keys = machine
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from([
            "schema_version",
            "version",
            "revision",
            "git_commit",
            "dirty",
            "build"
        ])
    );
    assert_eq!(machine["schema_version"], "chigiri/generator-info-v1");
    let commit = if build["commit"] == "unknown" {
        Value::Null
    } else {
        build["commit"].clone()
    };
    let dirty = if commit.is_null() {
        Value::Null
    } else {
        match build["dirty"].as_str().unwrap() {
            "clean" => Value::Bool(false),
            "dirty" => Value::Bool(true),
            "unknown" => Value::Null,
            value => panic!("Unexpected build dirty state: {value}"),
        }
    };
    assert_eq!(machine["version"], build["version"]);
    assert_eq!(machine["git_commit"], commit);
    assert_eq!(machine["dirty"], dirty);
    assert_eq!(machine["build"]["target"], build["target"]);
    assert_eq!(machine["build"]["profile"], build["profile"]);

    for arguments in [
        &[][..],
        &["--help"],
        &["--version"],
        &["sbom"],
        &["mock", "--version"],
    ] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let output = command(&copied, &runtime, arguments)
            .stdout(Stdio::from(writer))
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "Closed stdout: {arguments:?}"
        );
    }
    assert!(
        snapshot(&runtime) == before,
        "Offline metadata commands changed runtime files"
    );

    let stage = fixture.0.join("stage");
    let mut install = Command::new("make");
    install
        .current_dir(&root)
        .arg("install")
        .arg(variable("DESTDIR", &stage))
        .arg("PREFIX=/opt/craftguild")
        .arg(variable(
            "CARGO_TARGET_DIR",
            context["target_dir"].as_str().unwrap(),
        ))
        .arg(variable("PROFILE", context["profile"].as_str().unwrap()))
        .arg(variable(
            "VARIANT",
            context["variant"].as_str().unwrap_or(""),
        ));
    if context["explicit_target"] == true {
        install.arg(variable("TARGET", context["target"].as_str().unwrap()));
    }
    let installed = install.output().unwrap();
    succeeded(&installed, &["make", "install"]);
    assert!(
        fs::read(stage.join("opt/craftguild/bin/chigiri")).unwrap() == fs::read(binary).unwrap(),
        "Installed bytes differ from the built artifact"
    );
}
