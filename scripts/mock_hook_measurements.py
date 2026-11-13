"""Measure six-slot hook configurations with the same small HTTP query."""

from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import json
import http.client
import os
import re
import signal
import statistics
import subprocess
import time


QUERY = "query HookCost { greeting notes { id title } }"
EXPECTED = {"data": {"greeting": "hello", "notes": [
    {"id": "note-1", "title": "Japanese notes."}, {"id": "note-2", "title": "Second one."}]}}
INTENTIONAL_ERROR = '''        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })'''


def implement(source, body):
    if source.count(INTENTIONAL_ERROR) != 1:
        raise ValueError("Generated resolver does not contain the canonical intentional error.")
    return source.replace(INTENTIONAL_ERROR, "        " + body)


def cli(base, binary, label, *arguments):
    with (base / (label + ".log")).open("wb") as log:
        subprocess.run([str(binary), "mock", *arguments], cwd=base, stdin=subprocess.DEVNULL,
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=30)


def prepare(root, directory, binary, enabled):
    base = directory / ("hooks-enabled" if enabled else "hooks-disabled")
    base.mkdir()
    cli(base, binary, "init", "init", "--schema", str(root / "crates/mock/fixtures/hook_measure.graphql"),
        "--config-dir", "mock")
    project = base / "mock"
    config = (project / "mock.luau").read_text().replace("127.0.0.1:4000", "127.0.0.1:0")
    if enabled:
        edits = "\n".join(f"config.hooks.{phase}.{timing} = true"
                          for phase in ["request", "operation", "field"] for timing in ["before", "after"])
        config = config.replace("return config", edits + "\nreturn config")
    (project / "mock.luau").write_text(config)
    cli(base, binary, "update", "update", "--config-dir", "mock")
    greeting = project / "resolvers/query/greeting.luau"
    greeting.write_text(implement(greeting.read_text(), 'return "hello"'))
    notes = project / "resolvers/query/notes.luau"
    notes.write_text(implement(notes.read_text(), 'return ctx.models.Note:find({})'))
    (project / "seeds/Note.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n"
                                                   for row in EXPECTED["data"]["notes"]))
    if enabled:
        for hook in (project / "hooks").glob("*/*.luau"):
            hook.write_text(hook.read_text().replace("return nil", 'ctx.log("debug", "hook-cost")\n        return nil'))
    cli(base, binary, "check", "check", "--config-dir", "mock")
    return base


def distribution(values):
    return {"samples_seconds": values, "median_seconds": statistics.median(values),
            "minimum_seconds": min(values), "maximum_seconds": max(values)}


def concurrent_operation(port):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=8)
    try:
        connection.request("POST", "/graphql", json.dumps({"query": QUERY}), {"Content-Type": "application/json"})
        response = connection.getresponse()
        body = response.read()
        # An admission rejection may have an empty body. Do not confuse the
        # actual status under load with a queue failure or malformed JSON.
        parsed = json.loads(body) if body else None
        assert response.status != 200 or parsed is not None
        return {"status": response.status, "body": parsed, "bytes": len(body)}
    finally:
        connection.close()


def measure_server(base, binary, enabled, rss, operation, sample_operation):
    out, err = base / "server.stdout", base / "server.stderr"
    with out.open("wb") as stdout, err.open("wb") as stderr:
        started = time.monotonic()
        process = subprocess.Popen([str(binary), "mock", "serve", "--config-dir", "mock"], cwd=base,
                                   stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        try:
            startup_rss = []
            while True:
                if process.poll() is not None:
                    raise ValueError(f"measurement server exited: {err.read_text()}")
                startup_rss.append(rss(process.pid))
                found = re.search(r"chigiri mock listening http://127\.0\.0\.1:(\d+)", err.read_text())
                if found:
                    port = int(found[1])
                    break
                if time.monotonic() - started > 10:
                    raise ValueError("hook comparison startup watchdog expired")
                time.sleep(0.005)
            result = {"enabled": enabled, "startup_seconds": time.monotonic() - started,
                      "startup_sampled_peak_rss_bytes": max(startup_rss), "ready_rss_bytes": rss(process.pid)}
            # The first time we compare the run times of the entire phase separately, we go to the clock after the logger is exhausted.
            assert operation(port, QUERY) == EXPECTED
            deadline = time.monotonic() + 3
            while True:
                records = [json.loads(line) for line in out.read_bytes().split(b"\n")[:-1]]
                accesses = [record for record in records if record["kind"] == "access"]
                if accesses:
                    request_id = accesses[0]["request_id"]
                    hooks = [record for record in records if record["kind"] == "hook" and record["request_id"] == request_id]
                    if len(hooks) == (16 if enabled else 0):
                        break
                if time.monotonic() > deadline:
                    raise ValueError("isolated phase-count probe was not delivered completely")
                time.sleep(0.01)
            counts = Counter(record["phase"] + "." + record["timing"] for record in hooks)
            assert counts == (Counter({"request.before": 1, "request.after": 1, "operation.before": 1,
                                      "operation.after": 1, "field.before": 6, "field.after": 6}) if enabled else Counter())
            result["hook_invocations_per_operation"] = dict(counts)
            result["business_field_invocations"] = 6
            for _ in range(5):
                assert operation(port, QUERY) == EXPECTED
            times = []
            for _ in range(20):
                started = time.monotonic()
                response = operation(port, QUERY)
                times.append(time.monotonic() - started)
                assert response == EXPECTED
            result["sequential"] = distribution(times)
            result["sampled_operation"] = sample_operation(process.pid, port, QUERY)
            assert result["sampled_operation"]["response"] == EXPECTED
            # The value of the PS extraction is not called the pure HTTP response time.
            result["sampled_operation"]["timing_note"] = "includes sampler/thread overhead; latency comparison uses sequential"
            with ThreadPoolExecutor(max_workers=8) as pool:
                started = time.monotonic()
                responses = list(pool.map(lambda _: concurrent_operation(port), range(8)))
                result["concurrent_8"] = {"seconds": time.monotonic() - started,
                                          "successes": sum(response["status"] == 200 and response["body"] == EXPECTED for response in responses),
                                          "responses": responses}
            assert operation(port, QUERY) == EXPECTED, "server did not recover after concurrent load"
            stopping = time.monotonic()
            process.send_signal(signal.SIGTERM)
            assert process.wait(timeout=5) == 0 and "worker joined" in err.read_text()
            result["shutdown_seconds"] = time.monotonic() - stopping
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
    records = [json.loads(line) for line in out.read_text().splitlines()]
    result["log_records"] = dict(Counter(record["kind"] for record in records))
    result["log_loss"] = [record["dropped"] for record in records if record["kind"] == "log_loss"]
    return result


def measure_hooks(root, directory, binary, target, rss, operation, sample_operation):
    cases = [measure_server(prepare(root, directory, binary, enabled), binary, enabled,
                            rss, operation, sample_operation) for enabled in [False, True]]
    command = ["cargo", "+1.95.0", "test", "--locked", "--release", "-p", "chigiri_mock",
               "--lib", "hk_27", "--", "--nocapture"]
    with (directory / "hook-queue.log").open("wb") as log:
        subprocess.run(command, cwd=root, env=dict(os.environ, CARGO_TARGET_DIR=str(target)),
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    queue = [json.loads(line.removeprefix("HOOK_QUEUE_MEASURE "))
             for line in (directory / "hook-queue.log").read_text().splitlines()
             if line.startswith("HOOK_QUEUE_MEASURE ")]
    assert len(queue) == 2
    return {"query": QUERY, "warmup_requests": 5, "measured_requests": 20, "cases": cases,
            "queue_command": command, "queue": queue,
            "note": "same release CLI; six enabled slots, one explicit log per invocation (16 per operation); queue sampled separately in release lib test; no public telemetry API"}
