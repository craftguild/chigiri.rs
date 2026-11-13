//! Verify checked snapshots retain only initialized owned values and enforce bundle capacity across all inputs.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use crate::checked::{CheckError, CheckedMock};
use crate::ir::{MockIr, ScriptId};
use crate::luau::{MAX_BODY, ScriptCandidate, render_module};
use crate::output::OutputValue;
use crate::project::ServerSettings;
use crate::service::Worker;
use crate::store::SeedData;

fn server() -> ServerSettings {
    ServerSettings {
        listen: "127.0.0.1:0".parse().unwrap(),
        allowed_origins: vec!["https://example.test".into()],
    }
}

fn ir(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", source)]).unwrap())
}

#[test]
fn mp_06_20_26_checked_snapshot_passes_same_bytes_and_store_without_initializing_again() {
    let _test_case = crate::vm_gate::test_case();
    let ir = ir("type Query { value: Int! }");
    let source = "local calls = 0\nreturn {resolve=function(): number calls += 1 return calls end}";
    let build = || {
        CheckedMock::check(
            server(),
            ir.clone(),
            vec![ScriptCandidate {
                script: ScriptId(0),
                source: source.into(),
            }],
            SeedData::json("{}").unwrap(),
        )
        .unwrap()
    };
    let checked = build();
    let bundle = checked.bundle().to_owned();
    assert_eq!(bundle, build().bundle());
    let document = bundle.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["format"].as_str().unwrap(), "chigiri-mock/v11");
    assert_eq!(
        document["server"]["listen"].as_str().unwrap(),
        "127.0.0.1:0"
    );
    assert_eq!(
        document["resolvers"]["Query.value"]["luau"]
            .as_str()
            .unwrap(),
        source
    );
    assert!(document["resolvers"]["Query.value"].get("state").is_none());
    assert!(!bundle.contains("type Chigiri"));
    assert!(!bundle.contains("update-state"));
    let crate::checked::RuntimeInputs {
        scripts,
        hooks,
        store,
        ..
    } = checked.into_runtime();
    let lease = crate::vm_gate::test_guard();
    // Hold admission through startup so initialization cannot re-enter the checker or VM gate.
    let worker = Worker::from_checked(scripts, hooks, store).unwrap();
    drop(lease);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let field = ir.scripts()[0].field;
    for _ in 0..2 {
        let output = runtime
            .block_on(worker.client.execute(field, Object::new(), None))
            .unwrap();
        assert!(matches!(output, OutputValue::Scalar(Value::Int(1))));
    }
    drop(worker);
}

#[test]
fn cv_03_common_check_requires_types_and_initialization_without_calling_resolve() {
    let _test_case = crate::vm_gate::test_case();
    let ir = ir("type Query { value: Int! }");
    for (source, phase) in [
        ("return {resolve=true}", "type"),
        (
            "return {resolve=function(): string return 'wrong' end}",
            "type",
        ),
        ("error('private initializer')", "runtime"),
        ("return ({} :: any)", "module"),
    ] {
        let error = CheckedMock::check(
            server(),
            ir.clone(),
            vec![ScriptCandidate {
                script: ScriptId(0),
                source: source.into(),
            }],
            SeedData::json("{}").unwrap(),
        )
        .err()
        .unwrap();
        let CheckError::Script(error) = error else {
            panic!("script check required");
        };
        assert_eq!(error.phase, phase);
        assert!(!error.to_string().contains("private"));
    }
    CheckedMock::check(
        server(),
        ir,
        vec![ScriptCandidate {
            script: ScriptId(0),
            source: "return {resolve=function(): number error('must not run') end}".into(),
        }],
        SeedData::json("{}").unwrap(),
    )
    .unwrap();
}

#[test]
fn mp_20_27_31_common_snapshot_has_inclusive_canonical_bundle_limit() {
    let _test_case = crate::vm_gate::test_case();
    let fields = (0..8)
        .map(|id| format!("v{id}: Int!\n"))
        .collect::<String>();
    let ir = ir(&format!("type Query {{ {fields} }}"));
    let sources = (0..8)
        .map(|id| render_module(&ir, ScriptId(id), "return 42").unwrap())
        .collect::<Vec<_>>();
    let check = |sources: &[String]| {
        CheckedMock::check(
            server(),
            ir.clone(),
            sources
                .iter()
                .enumerate()
                .map(|(id, source)| ScriptCandidate {
                    script: ScriptId(id),
                    source: source.clone(),
                })
                .collect(),
            SeedData::json("{}").unwrap(),
        )
    };
    let baseline = check(&sources).unwrap();
    let mut remaining = crate::limits::CONFIG_BYTES - baseline.bundle().len();
    let mut padded = sources;
    for source in &mut padded {
        let padding = remaining.min(MAX_BODY - source.len());
        source.push_str(&" ".repeat(padding));
        remaining -= padding;
    }
    assert_eq!(remaining, 0);
    let exact = check(&padded).unwrap();
    assert_eq!(exact.bundle().len(), crate::limits::CONFIG_BYTES);
    padded
        .iter_mut()
        .find(|source| source.len() < MAX_BODY)
        .unwrap()
        .push(' ');
    assert!(matches!(check(&padded), Err(CheckError::Limit(_))));
}

#[test]
fn sa_10_export_contains_resolved_seed_snapshots() {
    let _test_case = crate::vm_gate::test_case();
    let ir =
        ir("type User{id:ID! name:String!} type Copy{id:ID! user:User!} type Query{copy:Copy}");
    let seed = SeedData::json(
        r#"{"Copy":[{"id":"c","user":{"$ref":{"model":"User","key":{"id":"u"}}}}],"User":[{"id":"u","name":"Akari"}]}"#,
    )
    .unwrap();
    let source = render_module(&ir, ScriptId(0), "return ctx.null").unwrap();
    let checked = CheckedMock::check(
        server(),
        ir,
        vec![ScriptCandidate {
            script: ScriptId(0),
            source,
        }],
        seed,
    )
    .unwrap();
    let document = checked.bundle().parse::<toml_edit::DocumentMut>().unwrap();
    let seed = document["seed"]["records_json"].as_str().unwrap();
    assert!(!seed.contains("$ref"));
    assert!(seed.contains(r#""user":{"id":"u","name":"Akari"}"#));
}
