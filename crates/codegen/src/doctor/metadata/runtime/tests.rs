//! Verify runtime identities from explicit Cargo metadata-v1 inputs, without Git history.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use super::super::Metadata;
use crate::{CodegenError, ResolvedManifest};

static NEXT: AtomicU64 = AtomicU64::new(0);
const FIRST: &str = "1111111111111111111111111111111111111111";
const SECOND: &str = "2222222222222222222222222222222222222222";
const URL: &str = "https://chigiri-doctor-test.invalid/runtime.git";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sibling-generator")
            .join(format!(
                "doctor-metadata-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        for directory in ["graphql", "sdk/src", "consumer/src"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        Self(fs::canonicalize(root).unwrap())
    }

    fn prepare(&self, revision: &str, url: &str) -> ResolvedManifest {
        fs::write(
            self.0.join("graphql/schema.graphql"),
            "type Query { value: String }",
        )
        .unwrap();
        fs::write(self.0.join("chigiri.toml"),format!("schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n[package]\nname=\"sdk\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[generation]\noutput=\"sdk\"\n[runtime]\nsource=\"git\"\nurl={url:?}\nrevision={revision:?}\n[consumer]\ncargo_manifest=\"consumer/Cargo.toml\"\npackage=\"consumer\"\ngenerated_dependency=\"contract\"\n")).unwrap();
        fs::write(self.0.join("sdk/Cargo.toml"),format!("[package]\nname=\"sdk\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nchigiri={{git={url:?},rev={revision:?}}}\n")).unwrap();
        fs::write(self.0.join("sdk/src/lib.rs"), "//! Metadata fixture SDK.\n").unwrap();
        fs::write(
            self.0.join("consumer/src/lib.rs"),
            "//! Metadata fixture consumer.\n",
        )
        .unwrap();
        fs::write(self.0.join("consumer/Cargo.toml"),format!("[package]\nname=\"consumer\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\ncontract={{package=\"sdk\",path=\"../sdk\"}}\nchigiri={{git={url:?},rev={revision:?}}}\n")).unwrap();
        ResolvedManifest::load(&self.0.join("chigiri.toml")).unwrap()
    }

    fn package(
        &self,
        id: &str,
        name: &str,
        directory: &str,
        source: Option<&str>,
        dependencies: Value,
    ) -> Value {
        json!({
            "id":id, "name":name, "version":"0.1.0", "source":source,
            "manifest_path":self.0.join(directory).join("Cargo.toml"),
            "targets":[{"src_path":self.0.join(directory).join("src/lib.rs")}],
            "features":{}, "dependencies":dependencies
        })
    }

    fn graph(&self, transport_url: &str, revision: &str) -> Value {
        // These are declared source-ID fixtures, not recorded fetches or real commits.
        let requested = format!("git+{transport_url}?rev={revision}");
        let resolved = format!("{requested}#{revision}");
        let runtime = json!([{
            "name":"chigiri", "rename":null, "kind":null, "source":requested
        }]);
        json!({
            "version":1, "workspace_members":["consumer"],
            "workspace_root":self.0.join("consumer"),
            "target_directory":self.0.join("target"),
            "packages":[
                self.package("consumer", "consumer", "consumer", None, json!([
                    {"name":"sdk", "rename":"contract", "kind":null, "source":null},
                    {"name":"chigiri", "rename":null, "kind":null, "source":requested}
                ])),
                self.package("sdk", "sdk", "sdk", None, runtime),
                self.package("facade", "chigiri", "runtime/crates/facade", Some(&resolved), json!([
                    {"name":"chigiri_core", "rename":null, "kind":null, "source":requested}
                ])),
                self.package("core", "chigiri_core", "runtime/crates/core", Some(&resolved), json!([]))
            ],
            "resolve":{"root":"consumer", "nodes":[
                {"id":"consumer", "deps":[
                    {"name":"contract", "pkg":"sdk", "dep_kinds":[{"kind":null}]},
                    {"name":"chigiri", "pkg":"facade", "dep_kinds":[{"kind":null}]}
                ]},
                {"id":"sdk", "deps":[
                    {"name":"chigiri", "pkg":"facade", "dep_kinds":[{"kind":null}]}
                ]},
                {"id":"facade", "deps":[
                    {"name":"chigiri_core", "pkg":"core", "dep_kinds":[{"kind":null}]}
                ]},
                {"id":"core", "deps":[]}
            ]}
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn metadata(graph: &Value) -> Metadata {
    Metadata::read(&serde_json::to_vec(graph).unwrap()).unwrap()
}

fn verify(manifest: &ResolvedManifest, graph: &Metadata) -> Result<(), CodegenError> {
    let consumer = manifest.manifest().consumer().unwrap();
    super::verify(graph, manifest, consumer, graph.selected(consumer).unwrap())
}

#[test]
fn sg_cli_05_accepts_matching_cargo_ssh_and_https_source_identities() {
    for (url, transport) in [
        (
            "ssh://git@chigiri-doctor-test.invalid:2222/runtime.git",
            "ssh://git@chigiri-doctor-test.invalid:2222/runtime.git",
        ),
        ("https://CHIGIRI-DOCTOR-TEST.invalid:443/runtime.git", URL),
    ] {
        let fixture = Fixture::new();
        let manifest = fixture.prepare(FIRST, url);
        let graph = metadata(&fixture.graph(transport, FIRST));
        verify(&manifest, &graph).unwrap();
        let sdk = graph.direct("consumer", "contract", false).unwrap();
        assert_eq!(
            super::requested_source(sdk).unwrap(),
            format!("git+{transport}?rev={FIRST}")
        );
    }
}

#[test]
fn sg_cli_05_rejects_mixed_cargo_git_sources_and_revisions() {
    let fixture = Fixture::new();
    let manifest = fixture.prepare(FIRST, URL);
    let valid = fixture.graph(URL, FIRST);
    verify(&manifest, &metadata(&valid)).unwrap();

    for source in [
        Value::Null,
        json!(format!("git+{URL}?rev={FIRST}#{SECOND}")),
        json!(format!("git+{URL}?rev={SECOND}#{SECOND}")),
        json!(format!(
            "git+https://other.example.invalid/runtime.git?rev={FIRST}#{FIRST}"
        )),
    ] {
        let mut graph = valid.clone();
        graph["packages"][3]["source"] = source.clone();
        assert!(verify(&manifest, &metadata(&graph)).is_err(), "{source}");
    }

    let mut graph = valid;
    graph["packages"][1]["dependencies"][0]["source"] = json!(format!("git+{URL}?rev={SECOND}"));
    assert!(verify(&manifest, &metadata(&graph)).is_err());
}

#[test]
fn sg_cli_05_rejects_duplicate_runtime_identities() {
    let fixture = Fixture::new();
    let manifest = fixture.prepare(FIRST, URL);
    let mut graph = fixture.graph(URL, FIRST);
    let mut other = graph["packages"][3].clone();
    other["id"] = json!("other-core");
    other["source"] = json!(format!("git+{URL}?rev={SECOND}#{SECOND}"));
    graph["packages"].as_array_mut().unwrap().push(other);
    graph["resolve"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"other-core", "deps":[]}));
    graph["resolve"]["nodes"][0]["deps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"other_core", "pkg":"other-core", "dep_kinds":[{"kind":null}]}));
    assert!(verify(&manifest, &metadata(&graph)).is_err());
}

#[test]
fn sg_cli_05_requires_complete_matching_git_revisions() {
    let requested = format!("git+{URL}?rev={FIRST}");
    let source = format!("{requested}#{FIRST}");
    assert!(super::git_source(&source, &requested, FIRST).unwrap());
    assert!(!super::git_source(&source, &requested, SECOND).unwrap());
    assert!(super::git_source(&requested, &requested, FIRST).is_err());
    assert!(super::git_source(&source, &format!("git+{URL}"), FIRST).is_err());
    assert!(!super::git_source("registry+https://example.invalid", &requested, FIRST).unwrap());
}
