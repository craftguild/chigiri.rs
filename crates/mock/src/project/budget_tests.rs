//! Verify exact management-metadata capacity at the limit and one unit beyond it.

use luaur::{Function, Lua, Table, Value};

use super::management::Management;
use super::metadata::MAX_BYTES;
use super::test_support::Fixture;
use super::update_test_support::{schema, state};

const CONFIG: &str = r#"
return {
    hooks = {request = {before = false, after = false}, operation = {before = false, after = false}, field = {before = false, after = false}},
    state = {fields = {}, initial = {}},
    format="chigiri-mock-project/v10", schema={sources={"a"}}, storage={sources={}}, clock={mode="system"},
    server={listen="127.0.0.1:0", allowed_origins={}},
}
"#;

fn settings(lua: &Lua) -> (Table, Table, Function) {
    let config: Table = lua.load(CONFIG).eval().unwrap();
    let server: Table = config.raw_get("server").unwrap();
    let origins: Table = server.raw_get("allowed_origins").unwrap();
    let strings: Table = lua.globals().get("string").unwrap();
    (config, origins, strings.get("len").unwrap())
}

#[test]
fn mp_31_config_owned_node_budget_is_shared_across_all_fields_and_array_items() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let (config, origins, length) = settings(&lua);
    // Six slot registration, schema/storage source tables, clock, and empty
    // state declaration/initial values use 51 nodes before origin entries.
    let count = crate::value::MAX_NODES - 51;
    for index in 1..=count {
        origins.raw_set(index, "http://x").unwrap();
    }
    let result = super::config_decode::decode(
        &lua,
        Value::Table(config.clone()),
        length.clone(),
        super::FORMAT,
    )
    .unwrap();
    assert_eq!(result.server.allowed_origins.len(), count);
    origins.raw_set(count + 1, "http://x").unwrap();
    let error = super::config_decode::decode(&lua, Value::Table(config), length, super::FORMAT)
        .err()
        .unwrap();
    assert_eq!(error.phase, "limit");
}

#[test]
fn mp_31_config_owned_bytes_are_charged_before_repeated_interned_strings_are_copied() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let (config, origins, length) = settings(&lua);
    let overhead = 20
        + [
            "format",
            "schema",
            "storage",
            "clock",
            "mode",
            "system",
            "server",
            "sources",
            "sources",
            "listen",
            "allowed_origins",
            "hooks",
            "state",
            "request",
            "operation",
            "field",
            "before",
            "after",
            "before",
            "after",
            "before",
            "after",
            "fields",
            "initial",
            super::FORMAT,
            "a",
            "127.0.0.1:0",
        ]
        .iter()
        .map(|text| text.len())
        .sum::<usize>();
    // Reserve fixed work for decoding the closed configuration shape; use the remainder for strings.
    let remaining = crate::api::WORK_LIMIT - 4096;
    let count = remaining / 1024 - 1;
    let origin = format!("https://{}", "x".repeat(1024 - 8));
    for index in 1..=count {
        origins.raw_set(index, origin.as_str()).unwrap();
    }
    let last = format!("https://{}", "x".repeat(remaining - count * 1024 - 8));
    origins.raw_set(count + 1, last.as_str()).unwrap();
    let result = super::config_decode::decode(
        &lua,
        Value::Table(config.clone()),
        length.clone(),
        super::FORMAT,
    )
    .unwrap();
    assert!(
        result
            .server
            .allowed_origins
            .iter()
            .map(String::len)
            .sum::<usize>()
            + overhead
            <= crate::value::MAX_BYTES
    );
    origins
        .raw_set(count + 1, format!("{last}{}", "x".repeat(8192)))
        .unwrap();
    let error = super::config_decode::decode(&lua, Value::Table(config), length, super::FORMAT)
        .err()
        .unwrap();
    assert_eq!(error.phase, "limit");
}

#[test]
fn mp_31_metadata_aggregate_boundary_and_transaction_auxiliary_budget_reject_before_writes() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(&schema("first"));
    let directory = super::filesystem::Directory::open(&fixture.project).unwrap();
    let mut state = state(&fixture);
    let mut snapshot: super::state::Snapshot =
        serde_json::from_str(&fixture.read("generated/schema.snapshot.json")).unwrap();
    let original_types = fixture.read("generated/types.d.luau");
    let bytes = original_types.len()
        + fixture.read("generated/schema.snapshot.json").len()
        + fixture.read("generated/update-state.json").len();
    let types = format!("{original_types}--{}", "x".repeat(MAX_BYTES - bytes - 2));
    let (snapshot, state) = state.render_documents(&mut snapshot, &types).unwrap();
    assert_eq!(types.len() + snapshot.len() + state.len(), MAX_BYTES);
    fixture.write("generated/types.d.luau", &types);
    fixture.write("generated/schema.snapshot.json", &snapshot);
    fixture.write("generated/update-state.json", &state);
    Management::read(&directory).unwrap();
    // Metadata parsing and digest checks succeed at the exact limit; readiness separately rejects IR-derived type drift.
    assert!(super::read_project(&fixture.project).is_err());
    let error = super::update_project(&fixture.project).err().unwrap();
    assert_eq!(error.phase, "limit");
    assert!(
        !fixture
            .project
            .join(super::filesystem::TRANSACTION)
            .exists()
    );
    assert_eq!(fixture.read("generated/types.d.luau"), types);
    fixture.write("generated/update-state.json", &(state + " "));
    assert_eq!(Management::read(&directory).err().unwrap().phase, "limit");
}
