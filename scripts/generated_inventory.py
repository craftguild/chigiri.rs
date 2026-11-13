"""Map registered manifests to closed ownership markers and generated outputs."""

import hashlib
import ipaddress
import json
from pathlib import Path
import re
import tomllib

from implementation_rule_io import components, read_file, require, visit_tree

MARKER = "chigiri-generated.json"
REGISTRY = "scripts/generated-manifests.json"
MAX_MARKER = 1024 * 1024


def closed_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value)
        value[key] = item
    return value


def fields(value, names):
    require(isinstance(value, dict) and set(value) == set(names.split()))


def matches(pattern, value):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None


def digest(value):
    return hashlib.sha256(value).hexdigest()


def semver(value):
    if not isinstance(value, str) or not 0 < len(value) <= 256:
        return False
    pattern = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?"
    result = re.fullmatch(pattern, value)
    if not result or any(int(part) > 2**64 - 1 for part in result.groups()[:3]):
        return False
    for index, group in enumerate(result.groups()[3:]):
        if group is not None:
            for part in group.split("."):
                if not part or (index == 0 and part.isdigit() and len(part) > 1 and part[0] == "0"):
                    return False
    return True


def git_url(value):
    if (not isinstance(value, str) or len(value.encode()) > 4096
            or any(ord(char) < 33 or ord(char) == 127 for char in value)
            or any(char in value for char in "?#\\")):
        return False
    matched = re.fullmatch(r"(https|ssh)://([^/]+)/(.+)", value)
    if matched is None:
        return False
    scheme, authority, path = matched.groups()
    if any(part in ("", ".", "..") for part in path.split("/")) or re.search(r"%(?![0-9a-fA-F]{2})", path):
        return False
    if "@" in authority:
        user, authority = authority.split("@", 1)
        if scheme != "ssh" or not matches(r"[A-Za-z0-9_-]+", user):
            return False
    if authority.startswith("["):
        address, separator, suffix = authority.partition("]")
        if not separator or suffix and not suffix.startswith(":"):
            return False
        try:
            ipaddress.IPv6Address(address[1:])
        except ValueError:
            return False
        port = suffix.removeprefix(":")
    else:
        host, _, port = authority.partition(":")
        if not all(matches(r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?", part) for part in host.split(".")):
            return False
    return not port or matches(r"[0-9]+", port) and 0 < int(port) <= 65535


def generator(value):
    fields(value, "schema_version version revision git_commit dirty build")
    require(value["schema_version"] == "chigiri/generator-info-v1" and semver(value["version"]))
    commit, dirty = value["git_commit"], value["dirty"]
    if commit is None:
        require(dirty is None and value["revision"] == "unknown")
    else:
        require(matches(r"[0-9a-f]{40}", commit) and commit != "0" * 40 and type(dirty) is bool)
        require(value["revision"] == commit + ("-dirty" if dirty else ""))
    build = value["build"]
    fields(build, "rustc host target profile")
    for key in ("host", "target", "profile"):
        require(matches(r"[0-9A-Za-z_.-]{1,256}", build[key]))
    rustc = build["rustc"]
    if rustc != "unknown":
        require(isinstance(rustc, str) and len(rustc) <= 256)
        banner = re.fullmatch(r"rustc (\S+) \([0-9a-fA-F]{7,40} [0-9]{4}-[0-9]{2}-[0-9]{2}\)", rustc)
        require(banner is not None and semver(banner[1]))


def records(values, maximum, byte_limit):
    require(isinstance(values, list) and 0 < len(values) <= maximum)
    result, folded = {}, set()
    previous, total = "", 0
    for value in values:
        fields(value, "path bytes sha256")
        path = value["path"]
        components(path)
        require(path > previous and path.casefold() not in folded)
        previous = path
        folded.add(path.casefold())
        require(type(value["bytes"]) is int and 0 <= value["bytes"] <= byte_limit)
        require(matches(r"[a-f0-9]{64}", value["sha256"]))
        total += value["bytes"]
        result[path] = value
    return result, total


def marker(data, label):
    require(len(data) <= MAX_MARKER)
    value = json.loads(data, object_pairs_hook=closed_object)
    fields(value, "schema_version package abi generator runtime provenance files")
    require(value["schema_version"] == "chigiri/generated-crate-v1" and type(value["abi"]) is int and value["abi"] == 8)
    package = value["package"]
    fields(package, "name version edition")
    require(matches(r"[A-Za-z_-][A-Za-z0-9_-]{0,254}", package["name"]) and package["name"] != "_")
    require(semver(package["version"]) and package["edition"] == "2024")
    generator(value["generator"])
    runtime = value["runtime"]
    require(isinstance(runtime, dict))
    if runtime.get("source") == "path":
        fields(runtime, "source root")
        root = runtime["root"]
        require(isinstance(root, str) and not root.startswith("/"))
        components("/".join("parent" if part in (".", "..") else part for part in root.split("/")))
    else:
        fields(runtime, "source url revision")
        require(runtime["source"] == "git" and matches(r"[a-f0-9]{40}", runtime["revision"]) and runtime["revision"] != "0" * 40)
        require(git_url(runtime["url"]))
    provenance = value["provenance"]
    fields(provenance, "manifest_label manifest_sha256 schema_sha256 binding_sha256 sources")
    require(provenance["manifest_label"] == label)
    for key in ("manifest_sha256", "schema_sha256", "binding_sha256"):
        require(matches(r"[a-f0-9]{64}", provenance[key]))
    sources, total = records(provenance["sources"], 1024, 256 * 1024)
    require(total <= 16 * 1024 * 1024)
    require(all(path.endswith((".gql", ".graphql")) for path in sources))
    inventory, total = records(value["files"], 65535, 64 * 1024 * 1024)
    require(total + len(data) <= 64 * 1024 * 1024)
    require({"Cargo.toml", "src/lib.rs", "src/__chigiri/generator_info.rs"} <= inventory.keys())
    require(all(path == "Cargo.toml" or path.startswith("src/") and path.endswith(".rs") for path in inventory))
    return value, inventory


def output_roots(repository):
    """Resolve output roots from controlled manifests without claiming paths twice."""
    paths = json.loads(read_file(repository / REGISTRY, 64 * 1024), object_pairs_hook=closed_object)
    require(isinstance(paths, list) and len(paths) <= 1024)
    roots = {}
    previous = ""
    for path in paths:
        parts = components(path)
        require(path > previous)
        previous = path
        manifest = tomllib.loads(read_file(repository / path, 64 * 1024).decode("utf-8"))
        require(manifest.get("schema_version") == "chigiri/config-v2")
        require(set(manifest) <= {"schema_version", "schema", "package", "generation", "runtime", "consumer", "bindings"})
        fields(manifest["generation"], "output")
        output = manifest["generation"]["output"]
        components(output)
        relative = Path(*parts[:-1], output).as_posix()
        components(relative)
        folded = relative.casefold()
        for existing in roots:
            require(not (folded == existing.casefold() or folded.startswith(existing.casefold() + "/") or existing.casefold().startswith(folded + "/")))
        roots[relative] = parts[-1]
    return roots


def owned_rust(repository):
    owned = set()
    for relative, label in output_roots(repository).items():
        root = repository / relative
        marker_bytes = read_file(root / MARKER, MAX_MARKER)
        value, inventory = marker(marker_bytes, label)

        def inspect(path, data):
            if path == MARKER:
                require(data == marker_bytes)
                return
            require(path in inventory)
            record = inventory[path]
            require(len(data) == record["bytes"] and digest(data) == record["sha256"])
            text = data.decode("utf-8")
            if path == "Cargo.toml":
                cargo = tomllib.loads(text)
                package = cargo["package"]
                require(all(package[key] == expected for key, expected in value["package"].items()))
                require(package["publish"] is False and package["rust-version"] == "1.95" and isinstance(cargo["workspace"], dict))

        files, directories = visit_tree(root, inspect)
        expected_directories = {str(parent) for path in inventory for parent in Path(path).parents if str(parent) != "."}
        require(files == inventory.keys() | {MARKER} and directories == expected_directories)
        owned.update(f"{relative}/{path}" for path in inventory if path.endswith(".rs"))
    return owned
