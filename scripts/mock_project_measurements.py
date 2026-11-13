"""Measure deployment time, startup time, and RSS for 32-model projects and bundles."""

import hashlib
import json
import signal
import subprocess
import time
import tomllib
import urllib.request


INTENTIONAL_ERROR = '''        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })'''


def implement(source, body):
    if source.count(INTENTIONAL_ERROR) != 1:
        raise ValueError("Generated resolver does not contain the canonical intentional error.")
    replacement = "        " + body.replace("\n", "\n        ")
    return source.replace(INTENTIONAL_ERROR, replacement)


def large_schema(argument):
    models = "\n".join(f"type Model{i} {{id:ID! name:String! enabled:Boolean}}" for i in range(32))
    fields = "\n".join(f'items{i}({argument}:Int! = 20):[Model{i}!]!' for i in range(32))
    return models + "\ntype Query {\n" + fields + "\n}\n"


def command(directory, binary, label, arguments):
    started = time.monotonic()
    argv = [str(binary), "mock", *map(str, arguments)]
    with (directory / (label + ".stdout")).open("wb") as stdout, (directory / (label + ".stderr")).open("wb") as stderr:
        result = subprocess.run(argv, stdout=stdout, stderr=stderr, timeout=300)
    if result.returncode:
        raise ValueError(f"Project measurement command failed: {label}, exit={result.returncode}")
    measured = {"argv": argv, "seconds": time.monotonic() - started, "exit": result.returncode}
    print(json.dumps({"project_step": label, **measured}), flush=True)
    return measured


def startup(directory, binary, label, arguments):
    started = time.monotonic()
    with (directory / (label + ".stdout")).open("wb") as stdout, (directory / (label + ".stderr")).open("wb") as stderr:
        process = subprocess.Popen(
            [str(binary), "mock", "serve", *map(str, arguments)],
            stdin=subprocess.DEVNULL,
            stdout=stdout,
            stderr=stderr,
        )
        try:
            samples = []
            while True:
                if process.poll() is not None:
                    raise ValueError(f"Listen to the end before: {label}")
                sample = subprocess.run(["ps", "-o", "rss=", "-p", str(process.pid)], capture_output=True, text=True, check=True, timeout=2)
                samples.append(int(sample.stdout.strip()) * 1024)
                output = next((line for line in (directory / (label + ".stderr")).read_text().splitlines() if line.startswith("chigiri mock listening http://")), "")
                if output.startswith("chigiri mock listening http://"):
                    elapsed = time.monotonic() - started
                    address = output.removeprefix("chigiri mock listening ")
                    break
                if time.monotonic() - started >= 300:
                    raise ValueError(f"Startup watchdog exceeded: {label}")
                time.sleep(.02)
            request = urllib.request.Request(address, data=b'{"query":"{__typename}"}', headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=8) as response:
                payload = json.loads(response.read())
            if payload != {"data": {"__typename": "Query"}}:
                raise ValueError(f"HTTP response mismatch after startup: {label}")
            process.send_signal(signal.SIGTERM)
            if process.wait(timeout=10) != 0 or "worker joined" not in (directory / (label + ".stderr")).read_text():
                raise ValueError(f"Worker recovery failed: {label}")
            measured = {"listen_seconds": elapsed, "startup_sampled_peak_rss_bytes": max(samples), "samples": len(samples), "http": payload, "worker_joined": True}
            print(json.dumps({"project_step": label, **measured}), flush=True)
            return measured
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def measure_projects(root, directory, binary):
    measured = {}
    for name, source in (("blog", (root / "crates/mock/fixtures/blog.graphql").read_text()), ("large", large_schema("limit"))):
        path = directory / (name + ".graphql")
        path.write_text(source)
        project = directory / name
        steps = {"init": command(directory, binary, name + "-init", ["init", "--schema", path, "--config-dir", project])}
        config = project / "mock.luau"
        config.write_text(config.read_text().replace("127.0.0.1:4000", "127.0.0.1:0"))
        if name == "large":
            path.write_text(large_schema("cap"))
            for label in ("update", "update_unchanged"):
                steps[label] = command(directory, binary, name + "-" + label, ["update", "--config-dir", project])
        # Supply implemented operations for successful-response measurements.
        if name == "blog":
            fixture = tomllib.loads((root / "crates/mock/fixtures/mock-v11-ready.toml").read_text())
            for coordinate, resolver in fixture["resolvers"].items():
                parent, field = coordinate.split(".")
                (project / "resolvers" / parent.lower() / (field + ".luau")).write_text(resolver["luau"])
        else:
            for index in range(32):
                resolver = project / "resolvers/query" / f"items{index}.luau"
                body = f'return ctx.models.Model{index}:find({{limit=args.cap}})'
                resolver.write_text(implement(resolver.read_text(), body))
        steps["check"] = command(directory, binary, name + "-check", ["check", "--config-dir", project])
        bundle = directory / (name + ".toml")
        steps["export"] = command(directory, binary, name + "-export", ["export", "--config-dir", project, "--output", bundle])
        steps["directory_start"] = startup(directory, binary, name + "-directory-start", ["--config-dir", project])
        steps["bundle_start"] = startup(directory, binary, name + "-bundle-start", ["--config", bundle])
        files = [path for path in project.rglob("*") if path.is_file()]
        measured[name] = {"steps": steps, "models": len(list((project / "seeds").glob("*.jsonl"))), "resolvers": len(list((project / "resolvers").rglob("*.luau"))), "files": len(files), "project_bytes": sum(path.stat().st_size for path in files), "sdl_bytes": path.stat().st_size, "bundle_bytes": bundle.stat().st_size, "bundle_sha256": hashlib.sha256(bundle.read_bytes()).hexdigest()}
    return measured
