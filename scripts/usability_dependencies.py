"""Check neutral defaults while allowing the explicit optional worker runtime."""

import json
import sys
from pathlib import Path


def validate(metadata):
    packages = {package["name"]: package for package in metadata["packages"]}
    if packages["chigiri_core"]["dependencies"]:
        raise ValueError("An unexpected chigiri_core dependency was added.")
    facade = packages["chigiri"]
    dependencies = [entry for entry in facade["dependencies"] if entry["kind"] is None]
    if any(entry["name"] in {"axum", "hyper", "hyper-util", "tower"} for entry in dependencies):
        raise ValueError("The chigiri facade contains a server dependency.")
    runtime = [entry for entry in dependencies if entry["name"] == "tokio"]
    features = facade["features"]
    if (len(runtime) != 1 or runtime[0]["optional"] is not True
            or features.get("default") != []
            or "dep:tokio" not in features.get("subscription-worker", [])):
        raise ValueError("Tokio must remain an explicit optional subscription-worker dependency.")
    for name, values in features.items():
        if name != "subscription-worker" and any(
                value in {"tokio", "dep:tokio"} or value.startswith("tokio/") for value in values):
            raise ValueError("Only subscription-worker may activate Tokio directly.")
    pending, seen = ["graphiql"], set()
    while pending:
        name = pending.pop()
        if name == "subscription-worker":
            raise ValueError("GraphiQL must not activate the worker runtime.")
        if name not in seen:
            seen.add(name)
            pending.extend(features.get(name, []))
    if "chigiri_production" in packages:
        raise ValueError("The deleted chigiri_production package is still in the workspace.")
    if facade["description"] != "Application-facing GraphQL endpoint framework for Rust":
        raise ValueError("The chigiri facade metadata is not an application endpoint.")
    if packages["chigiri_http"]["description"] != "Runtime-neutral GraphQL over HTTP codec and lifecycle SPI for Chigiri":
        raise ValueError("The chigiri_http metadata is not codec/lifecycle SPI.")


if __name__ == "__main__":
    try:
        validate(json.loads(Path(sys.argv[1]).read_text()))
    except ValueError as error:
        raise SystemExit(str(error)) from error
