//! Test required metadata fields, identity references, and finite graph traversal.

use serde_json::{Value, json};

use super::Metadata;

fn package(id: &str) -> Value {
    json!({"id":id,"name":id,"version":"0.1.0","source":null,"manifest_path":format!("/workspace/{id}/Cargo.toml"),"targets":[{"src_path":format!("/workspace/{id}/src/lib.rs")}],"features":{},"dependencies":[]})
}

fn dependency(name: &str, package: &str, kind: Value) -> Value {
    json!({"name":name,"pkg":package,"dep_kinds":[{"kind":kind}]})
}

fn graph() -> Value {
    json!({"version":1,"packages":[package("app"),package("sdk"),package("build"),package("dev")],"workspace_members":["app"],"workspace_root":"/workspace","target_directory":"/workspace/target","resolve":{"root":"app","nodes":[
        {"id":"app","deps":[dependency("contract","sdk",Value::Null),dependency("helper","build","build".into()),dependency("tests","dev","dev".into())]},
        {"id":"sdk","deps":[]},{"id":"build","deps":[]},{"id":"dev","deps":[dependency("app","app",Value::Null)]}
    ]}})
}

fn read(value: &Value) -> Result<Metadata, crate::CodegenError> {
    Metadata::read(&serde_json::to_vec(value).unwrap())
}

#[test]
fn sg_proc_02_resolves_renames_and_dependency_kinds_without_mixing_build_dependencies() {
    let metadata = read(&graph()).unwrap();
    assert_eq!(
        metadata.direct("app", "contract", false).unwrap().name,
        "sdk"
    );
    assert!(metadata.direct("app", "sdk", false).is_err());
    assert!(metadata.direct("app", "helper", false).is_err());
    assert!(metadata.direct("app", "helper", true).is_err());
    assert!(metadata.direct("app", "tests", false).is_err());
    assert_eq!(metadata.direct("app", "tests", true).unwrap().name, "dev");
    assert_eq!(
        metadata
            .reachable("app", false)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        ["app", "sdk"]
    );
    assert_eq!(
        metadata
            .reachable("app", true)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        ["app", "dev", "sdk"]
    );
}

#[test]
fn sg_proc_02_rejects_missing_optional_duplicate_and_broken_graph_references() {
    for (pointer, value) in [
        ("/version", json!(2)),
        ("/packages/0/id", json!("sdk")),
        ("/workspace_members/0", json!("unknown")),
        ("/resolve/nodes/0/deps/0/pkg", json!("unknown")),
        ("/resolve/nodes/1/id", json!("app")),
        ("/packages/0/targets", json!([])),
        ("/target_directory", json!("relative")),
    ] {
        let mut graph = graph();
        *graph.pointer_mut(pointer).unwrap() = value;
        assert!(read(&graph).is_err(), "{pointer}");
    }
    let mut missing = graph();
    missing["packages"][0]
        .as_object_mut()
        .unwrap()
        .remove("source");
    assert!(read(&missing).is_err());
    let mut missing = graph();
    missing["resolve"]["nodes"][0]["deps"][0]["dep_kinds"][0]
        .as_object_mut()
        .unwrap()
        .remove("kind");
    assert!(read(&missing).is_err());
    let duplicate = graph()
        .to_string()
        .replace("\"version\":1", "\"version\":1,\"version\":1");
    assert!(Metadata::read(duplicate.as_bytes()).is_err());
}

#[test]
fn sg_proc_01_byte_and_instances_and_opaque_id_limits_are_rejected() {
    assert!(Metadata::read(&vec![b' '; 64 * 1024 * 1024 + 1]).is_err());
    let mut oversized = graph();
    oversized["workspace_members"] = json!(vec!["app"; 65_537]);
    assert!(read(&oversized).is_err());
    let mut oversized = graph();
    oversized["packages"][0]["id"] = "x".repeat(16 * 1024 + 1).into();
    assert!(read(&oversized).is_err());
}

#[test]
fn sg_proc_02_rejects_duplicate_aliases_when_the_graph_contains_an_sdk_name() {
    let mut value = graph();
    value["resolve"]["nodes"][0]["deps"]
        .as_array_mut()
        .unwrap()
        .push(dependency("contract", "dev", Value::Null));
    let metadata = read(&value).unwrap();
    assert!(metadata.direct("app", "contract", false).is_err());
}
