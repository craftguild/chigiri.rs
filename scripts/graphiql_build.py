"""Build GraphiQL assets from the pinned npm lock and verify exact drift."""

import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from implementation_rule_io import require

from graphiql_inventory import BASE, ROOT, digest, source_inventory


def content_type(name):
    if name.endswith(".js"):
        return "text/javascript; charset=utf-8"
    if name.endswith(".css"):
        return "text/css; charset=utf-8"
    if name.endswith(".ttf"):
        return "font/ttf"
    if name.endswith(".txt"):
        return "text/plain; charset=utf-8"
    raise ValueError("unregistered asset format")


def package_name(path):
    return path.rsplit("node_modules/", 1)[-1]


def license_inventory(work, metadata):
    lock = json.loads((work / "package-lock.json").read_text())
    supplements = {(item["name"], item["version"]): item for item in
                   json.loads((work / "license-supplements.json").read_text())}
    used_supplements = set()
    included = set()
    for name in metadata["inputs"]:
        if not name.startswith("node_modules/"):
            continue
        path = work / name
        while path.parent != work:
            manifest = path / "package.json"
            if manifest.is_file() and json.loads(manifest.read_text()).get("name"):
                included.add(path.relative_to(work).as_posix())
                break
            path = path.parent
    records, notices = [], ["Chigiri GraphiQL pinned third-party asset licenses and notices.\n"]
    for path, package in sorted(lock["packages"].items()):
        if not path:
            continue
        require(package["resolved"].startswith("https://registry.npmjs.org/"))
        record = {"name": package_name(path), "version": package["version"],
                  "license": package["license"], "bundled": path in included,
                  "integrity": package["integrity"]}
        records.append(record)
        directory = work / path
        if directory.is_dir():
            texts = [file for file in directory.iterdir() if file.is_file() and
                     re.search(r"license|copying|copyright|notice", file.name, re.I)]
            if not texts and path in included:
                key = record["name"], record["version"]
                if not (key in supplements):
                    raise ValueError(f"bundled dependency has no license text: {path}")
                supplement = supplements[key]
                require(record["license"] == supplement["license"] == "MIT")
                used_supplements.add(key)
                notices.extend([f"\n--- {record['name']} {record['version']} / upstream LICENSE ---\n",
                                supplement["source"], supplement["text"]])
            for file in sorted(texts):
                notices.extend([f"\n--- {record['name']} {record['version']} / {file.name} ---\n",
                                file.read_text(encoding="utf-8")])
    require(included <= set(lock["packages"]))
    require(used_supplements == set(supplements))
    return records, "\n".join(notices).encode()


def embedding(records):
    lines = ["//! Embedded assets generated from the pinned npm lock by graphiql_build.py. Do not edit manually.", "",
             "pub(super) const ASSETS: &[(&str, &str, &[u8])] = &["]
    for record in records:
        name = record["path"]
        lines.extend(["    (", f'        "{name}",', f'        "{record["content_type"]}",',
                      f'        include_bytes!("../assets/{name}"),', "    ),"])
    lines.extend([
        "];", "",
    ])
    return "\n".join(lines).encode()


def validate_audit(audit):
    vulnerabilities = audit["metadata"]["vulnerabilities"]
    require(set(vulnerabilities) == {"info", "low", "moderate", "high", "critical", "total"})
    require(all(value == 0 for value in vulnerabilities.values()))


def validate_imports(metadata):
    for output in metadata["outputs"].values():
        for imported in output.get("imports", []):
            # External imports are forbidden except inlined data images; workers
            # use pinned local URLs.
            require(not imported.get("external") or imported["path"].startswith("data:image/"))


def verify_outputs(base, outputs):
    for name, data in outputs.items():
        if (base / name).read_bytes() != data:
            raise ValueError(f"asset drift detected: {name}")


def build(check, evidence=False, reproduce=None, typecheck=False, prepare_dependencies=False):
    base = ROOT / BASE
    parent = ROOT / "target/graphiql-assets"
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="build-", dir=parent) as name:
        work = Path(name)
        for path in (base / "frontend").iterdir():
            require(path.is_file() and not path.is_symlink())
            shutil.copyfile(path, work / path.name)
        npm = ["npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund"]
        if not prepare_dependencies:
            npm.insert(2, "--offline")
        subprocess.run(npm,
                       cwd=work, check=True, timeout=300, stdout=sys.stderr if reproduce else None)
        if prepare_dependencies:
            print("Prepared locked GraphiQL npm cache; ordinary generation uses --offline.")
            return
        with (parent / "audit.json").open("wb") as output:
            subprocess.run(["npm", "audit", "--json"], cwd=work,
                           stdout=output, check=True, timeout=120)
        # Check every severity explicitly instead of inheriting the user's audit-level setting.
        audit = json.loads((parent / "audit.json").read_text())
        validate_audit(audit)
        if typecheck:
            tools = ROOT / "node_modules"
            subprocess.run([
                "node", str(tools / "typescript/bin/tsc"), "--noEmit", "--allowJs",
                "--checkJs", "--skipLibCheck", "--module", "esnext",
                "--moduleResolution", "bundler", "--target", "es2022",
                "--typeRoots", str(tools / "@types"), "entry.mjs", "build.mjs",
            ], cwd=work, check=True, timeout=120)
        subprocess.run(["node", "build.mjs"], cwd=work, check=True, timeout=120,
                       stdout=sys.stderr if reproduce else None)
        metadata = json.loads((work / "meta.json").read_text())
        validate_imports(metadata)
        licenses, notices = license_inventory(work, metadata)
        (work / "assets/THIRD_PARTY_NOTICES.txt").write_bytes(notices)
        records = [{"path": path.relative_to(work / "assets").as_posix(),
                    "bytes": path.stat().st_size, "sha256": digest(path.read_bytes()),
                    "content_type": content_type(path.name)}
                   for path in sorted((work / "assets").rglob("*")) if path.is_file()]
        embedded = embedding(records)
        license_bytes = (json.dumps(licenses, ensure_ascii=False, indent=2) + "\n").encode()
        inventory = {"format": "chigiri-graphiql-assets/v1", "sources": source_inventory(ROOT),
                     "files": records, "embedding": digest(embedded), "licenses": digest(license_bytes)}
        outputs = {f"assets/{record['path']}": (work / "assets" / record["path"]).read_bytes()
                   for record in records}
        outputs.update({"src/embedded.rs": embedded, "dependency-licenses.json": license_bytes,
                        "asset-inventory.json": (json.dumps(inventory, ensure_ascii=False, indent=2) + "\n").encode()})
        if check:
            from graphiql_inventory import check as verify
            verify()
            verify_outputs(base, outputs)
        else:
            # Only assets recorded in the previous verified inventory are
            # eligible for replacement or removal.
            previous = base / "asset-inventory.json"
            if previous.exists():
                from graphiql_inventory import check as verify
                verify(verify_sources=False)
                old = json.loads(previous.read_text())
                for record in old["files"]:
                    path = base / "assets" / record["path"]
                    if f"assets/{record['path']}" not in outputs:
                        path.unlink()
                directories = {parent for record in old["files"] for parent in
                               (base / "assets" / record["path"]).parents
                               if parent.is_relative_to(base / "assets") and parent != base / "assets"}
                for directory in sorted(directories, key=lambda path: len(path.parts), reverse=True):
                    if not any(directory.iterdir()):
                        directory.rmdir()
            else:
                if not (not (base / "assets").exists()):
                    raise ValueError("Refusing to replace an asset directory without a verified inventory.")
                if not (not (base / "src/embedded.rs").exists()):
                    raise ValueError("Refusing to replace an embedded asset table without a verified inventory.")
            for name, data in outputs.items():
                path = base / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
        action = "drift check" if check else "generation"
        print(f"GraphiQL asset {action} passed: {len(records)} files / {sum(r['bytes'] for r in records)} bytes",
              file=sys.stderr if reproduce else sys.stdout)
        if evidence:
            from graphiql_evidence import export
            export(work, metadata, ROOT)
        if reproduce is not None:
            require(reproduce.is_dir() and not reproduce.is_symlink())
            require(not any(reproduce.iterdir()))
            for record in records:
                target = reproduce / record["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("xb") as output:
                    output.write((work / "assets" / record["path"]).read_bytes())


if __name__ == "__main__":
    args = sys.argv[1:]
    if args == ["--prepare-dependencies"]:
        build(False, prepare_dependencies=True)
    elif len(args) == 2 and args[0] == "--reproduce":
        build(True, reproduce=Path(args[1]))
    else:
        require(args in ([], ["--check"], ["--check", "--evidence"], ["--check", "--typecheck"]))
        build("--check" in args, evidence="--evidence" in args, typecheck="--typecheck" in args)
