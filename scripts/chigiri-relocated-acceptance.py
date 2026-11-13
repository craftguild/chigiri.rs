#!/usr/bin/env python3
"""Exercise both workflows through one copied Chigiri binary outside the repository."""

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from validation_evidence import source_snapshot
from product_cli import built_cli


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE = ROOT / "target/chigiri-relocated/result.json"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command_environment(tools, cargo_target):
    cargo = shutil.which("cargo")
    rustup = shutil.which("rustup")
    if not cargo or not rustup:
        raise ValueError("Cargo and rustup are required for relocated Rust acceptance")
    tools.mkdir()
    os.symlink(cargo, tools / "cargo")
    os.symlink(rustup, tools / "rustup")
    system = [path for path in ("/usr/bin", "/bin", "/usr/sbin", "/sbin") if Path(path).is_dir()]
    path = os.pathsep.join([str(tools), *system])
    for retired in ("cargo-pythia", "pythia-mock"):
        if shutil.which(retired, path=path):
            raise ValueError(f"retired executable is present on the acceptance PATH: {retired}")
    return dict(
        os.environ,
        PATH=path,
        CARGO_NET_OFFLINE="true",
        CARGO_TARGET_DIR=str(cargo_target),
    )


def run(records, label, argv, cwd, env, expected=0, timeout=300):
    started = time.monotonic()
    result = subprocess.run(
        [str(value) for value in argv],
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=timeout,
    )
    if result.returncode != expected:
        raise ValueError(
            f"{label}: exit={result.returncode}; stdout={result.stdout.decode(errors='replace')}; "
            f"stderr={result.stderr.decode(errors='replace')}"
        )
    records.append(
        {
            "label": label,
            "argv": [str(value) for value in argv[1:]],
            "exit": result.returncode,
            "seconds": time.monotonic() - started,
        }
    )
    return result


def snapshot(root):
    result = {}
    for path in sorted(root.rglob("*")):
        if path.is_file():
            name = path.relative_to(root).as_posix()
            result[name] = digest(path)
    return result


def rust_workflow(binary, base, env):
    records = []
    project = base / "rust-project"
    project.mkdir()
    run(records, "rust-init", [binary, "rust", "init"], project, env)
    repository = json.dumps(os.path.relpath(ROOT, project))
    manifest = f'''schema_version = "chigiri/config-v2"
[schema]
directory = "graphql"
[package]
name = "relocated-contract"
version = "0.1.0"
edition = "2024"
[generation]
output = "generated/contract"
[runtime]
source = "path"
root = {repository}
[consumer]
cargo_manifest = "consumer/Cargo.toml"
package = "relocated-consumer"
generated_dependency = "relocated_contract"
features = []
all_features = false
no_default_features = false
all_targets = true
'''
    (project / "chigiri.toml").write_text(manifest)
    schema = project / "graphql/schema.graphql"
    schema.parent.mkdir()
    schema.write_text("type Query { book: Book! }\ntype Book { id: ID! title: String! }\n")
    prefix = [binary, "rust", "--manifest", "chigiri.toml"]
    run(records, "rust-codegen", [*prefix, "codegen", "--deny-warnings"], project, env)
    run(records, "rust-check", [*prefix, "check", "--deny-warnings"], project, env)

    before = snapshot(project)
    run(records, "rust-scaffold-rejected", [*prefix, "scaffold", "--object", "Book"], project, env, expected=2)
    if snapshot(project) != before:
        raise ValueError("retired scaffold rejection changed the project")

    consumer = project / "consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text(f'''[package]
name = "relocated-consumer"
version = "0.1.0"
edition = "2024"
publish = false
[workspace]
[dependencies]
relocated_contract = {{ package = "relocated-contract", path = "../generated/contract" }}
chigiri = {{ path = {json.dumps(str(ROOT / "crates/facade"))} }}
''')
    (consumer / "src/lib.rs").write_text(
        "pub struct Types;\n"
        "pub struct Book { pub id: chigiri::Id, pub title: String }\n"
        "impl relocated_contract::SchemaTypes for Types { type Book = Book; }\n"
        "pub struct BookFields;\n"
        "impl relocated_contract::BookResolver<(), Types, chigiri::FieldError> for BookFields {\n"
        "async fn id<'a>(&'a self, _: chigiri::FieldContext<'a, ()>, book: &'a Book) -> chigiri::FieldResult<chigiri::Id> { Ok(book.id.clone()) }\n"
        "async fn title<'a>(&'a self, _: chigiri::FieldContext<'a, ()>, book: &'a Book) -> chigiri::FieldResult<String> { Ok(book.title.clone()) }\n"
        "}\n"
    )
    run(
        records,
        "consumer-lock",
        ["cargo", "+1.95.0", "generate-lockfile", "--offline", "--manifest-path", "consumer/Cargo.toml"],
        project,
        env,
    )
    run(records, "rust-doctor", [*prefix, "doctor", "--deny-warnings"], project, env)
    schema.write_text(
        "type Query { book: Book! health: String! }\n"
        "type Book { id: ID! title: String! }\n"
    )
    run(records, "rust-update", [*prefix, "update", "--deny-warnings"], project, env)
    run(records, "rust-check-updated", [*prefix, "check", "--deny-warnings"], project, env)
    run(records, "rust-doctor-updated", [*prefix, "doctor", "--deny-warnings"], project, env)
    return {
        "steps": records,
        "generated_marker": json.loads(
            (project / "generated/contract/chigiri-generated.json").read_text()
        )["schema_version"],
        "resolver_form": "complete-traits",
    }


def mock_workflow(binary, base, env):
    result = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/mock-quickstart.py"),
            "--binary",
            str(binary),
            "--work-parent",
            str(base),
        ],
        cwd=ROOT,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=180,
    )
    if result.returncode != 0:
        raise ValueError(result.stderr.decode(errors="replace"))
    return json.loads(result.stdout)


def main():
    source = built_cli().resolve(strict=True)
    EVIDENCE.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="chigiri-relocated-") as directory:
        base = Path(directory).resolve()
        if base == ROOT or ROOT in base.parents:
            raise ValueError("relocated acceptance directory is inside the repository")
        binary = base / "bin/chigiri"
        binary.parent.mkdir()
        shutil.copy2(source, binary)
        binary.chmod(0o755)
        if digest(binary) != digest(source):
            raise ValueError("copied binary differs from the built product")
        env = command_environment(base / "tools", base / "cargo-target")
        rust = rust_workflow(binary, base, env)
        mock = mock_workflow(binary, base, env)
        if mock["binary_sha256"] != digest(binary):
            raise ValueError("Mock workflow did not use the relocated product binary")
        result = {
            "schema_version": "chigiri/relocated-acceptance-v1",
            "source_sha256": source_snapshot(ROOT)["sha256"],
            "binary_sha256": digest(binary),
            "copied_outside_repository": True,
            "retired_executables_on_path": [],
            "rust": rust,
            "mock": mock,
        }
    EVIDENCE.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(f"Relocated Chigiri acceptance passed: {EVIDENCE}")


if __name__ == "__main__":
    main()
