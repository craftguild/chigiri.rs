//! Verifies hook modules when enabled and portable snapshots containing only
//! initial state and enabled hooks.

use super::test_support::Fixture;
use super::{ActiveSnapshot, read_project, update_project};
use crate::hooks::{HookSlot, StateContract};
use crate::luau::render_hook_module;
use crate::value::ValueBudget;

const SDL: &str = include_str!("../../fixtures/blog.graphql");

#[test]
fn cv_03_reenabled_hook_requires_typed_closed_module_without_calling_run() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let original = fixture.read("mock.luau");
    let enabled = original.replace("request = {before = false", "request = {before = true");
    let path = HookSlot::RequestBefore.path();
    for (source, phase) in [
        ("return (", "compile"),
        ("local m:any={run=true} return m", "module"),
        (
            "local m:any={state='ready',run=function() return nil end} return m",
            "module",
        ),
        ("return {run=function(): number return 1 end}", "type"),
    ] {
        fixture.write("mock.luau", &original);
        fixture.write(path, source);
        update_project(&fixture.project).unwrap();
        read_project(&fixture.project).unwrap();
        fixture.write("mock.luau", &enabled);
        update_project(&fixture.project).unwrap();
        let failure = read_project(&fixture.project).err().unwrap();
        assert_eq!(failure.phase, phase);
        assert_eq!(failure.file, path);
        assert!(!failure.coordinate_known());
        assert_eq!(failure.hook_slot(), Some(HookSlot::RequestBefore));
        assert_eq!(fixture.read(path), source);
    }
    fixture.write(
        path,
        &render_hook_module(
            HookSlot::RequestBefore,
            "error('run must not be invoked by validation')",
        )
        .unwrap(),
    );
    let checked = read_project(&fixture.project).unwrap();
    assert_eq!(checked.hooks().len(), 1);
    assert_eq!(checked.hooks()[0].slot(), HookSlot::RequestBefore);
}

#[test]
fn hk_06_export_contains_initial_state_and_only_enabled_original_hook_bytes() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let config = fixture.read("mock.luau").replace("return config", r#"
config.state = {fields = {authenticated={kind="boolean"},user={kind="nullable",of={kind="string"}}},initial={authenticated=false}}
config.hooks.request.before = true
return config"#);
    fixture.write("mock.luau", &config);
    update_project(&fixture.project).unwrap();
    let body = "-- Preserve the user's formatting\nreturn {run=function(ctx)\nreturn {statePatch={user=ctx.null}}\nend}";
    fixture.write(HookSlot::RequestBefore.path(), body);
    fixture.write(HookSlot::FieldAfter.path(), "private disabled hook source");
    let checked = read_project(&fixture.project).unwrap();
    let document = checked.bundle().parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["format"].as_str(), Some("chigiri-mock/v11"));
    assert_eq!(document["hooks"]["request"]["before"].as_bool(), Some(true));
    assert_eq!(document["hooks"]["modules"].as_table().unwrap().len(), 1);
    assert_eq!(
        document["hooks"]["modules"]["request.before"]["luau"].as_str(),
        Some(body)
    );
    assert!(!checked.bundle().contains("private disabled"));
    let state = StateContract::from_json(
        document["state"]["contract_json"].as_str().unwrap(),
        &mut ValueBudget::default(),
    )
    .unwrap();
    assert_eq!(
        state.initial(),
        checked.scripts().contract().state().initial()
    );
    assert_eq!(
        state.fields(),
        checked.scripts().contract().state().fields()
    );
    assert_eq!(
        ActiveSnapshot::capture(&fixture.project)
            .unwrap()
            .schema()
            .settings()
            .hooks,
        checked.hook_settings()
    );
}
