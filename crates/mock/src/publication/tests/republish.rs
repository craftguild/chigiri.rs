//! Preserve a sealed capture through a fresh VM and a separately charged child publication.

use luaur::{Lua, Value as LuaValue};

use super::support::{fixture, record_output, replace_record};
use crate::codec::Codec;
use crate::output::{ObjectData, OutputValue};
use crate::publication::batch::{BatchBuilder, CapturedBatch};
use crate::publication::{PublicationLimits, PublicationStore};
use crate::value::ValueBudget;

fn captured_batch(fixture: &super::support::Fixture, output: &OutputValue) -> CapturedBatch {
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.changed, "original", output)
        .unwrap();
    builder.seal().unwrap()
}

#[test]
fn captured_record_round_trip_retains_original_identity_and_slots_after_store_replacement() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture();
    let (mut store, output) = record_output(&fixture, "original");
    let original = captured_batch(&fixture, &output);
    replace_record(&mut store, &fixture);
    let captured = original.publications()[0]
        .project(
            store.snapshot(),
            fixture.changed,
            &mut ValueBudget::default(),
        )
        .unwrap();
    let _guard = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let mut codec = Codec::new(&lua).unwrap();
    let encoded = codec.encode_output(&captured).unwrap();
    let ty = &fixture.ir.graph().fields()[fixture.changed.0].ty;
    let (value, provenance) = codec.decode_output(&fixture.ir, ty, &encoded).unwrap();
    let validated = crate::output::validate_provenance_detailed(
        &store.snapshot(),
        fixture.changed,
        &value,
        &provenance,
        &mut ValueBudget::default(),
    )
    .unwrap();
    assert!(
        matches!(&validated, OutputValue::Object(object) if matches!(object.data(), ObjectData::CapturedRecord(_)))
    );
    let world = PublicationStore::new(store, 1).unwrap();
    let (_, retention) = world.source_policy();
    let child = world
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    child
        .push(&fixture.ir, fixture.changed, "child", &validated)
        .unwrap();
    let prepared = child.prepare(&world.staging()).unwrap().unwrap();
    assert_eq!(
        original.publications()[0].payload(),
        prepared.retained.batch().publications()[0].payload()
    );
    assert!(retention.used() > 0);
    drop(prepared);
    assert_eq!(retention.used(), 0);
}

#[test]
fn copied_modified_and_foreign_captured_record_tables_never_gain_record_authority() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture();
    let (mut store, output) = record_output(&fixture, "original");
    let original = captured_batch(&fixture, &output);
    replace_record(&mut store, &fixture);
    let captured = original.publications()[0]
        .project(
            store.snapshot(),
            fixture.changed,
            &mut ValueBudget::default(),
        )
        .unwrap();
    let _guard = crate::vm_gate::test_guard();
    for body in [
        "return {__typename=event.__typename,id=event.id,name=event.name,data=event.data}",
        "event.name='modified' return event",
        "event.id='other' return event",
    ] {
        let lua = Lua::new();
        let mut codec = Codec::new(&lua).unwrap();
        let encoded = codec.encode_output(&captured).unwrap();
        lua.globals()
            .set("event", encoded.as_lua().clone())
            .unwrap();
        assert!(
            lua.load("return table.clone(event)")
                .eval::<LuaValue>()
                .is_err()
        );
        let returned: LuaValue = lua.load(body).eval().unwrap();
        let returned = codec.scope().callback(&lua, returned).unwrap();
        let (value, provenance) = codec
            .decode_output(
                &fixture.ir,
                &fixture.ir.graph().fields()[fixture.changed.0].ty,
                &returned,
            )
            .unwrap();
        assert!(
            crate::output::validate_provenance_detailed(
                &store.snapshot(),
                fixture.changed,
                &value,
                &provenance,
                &mut ValueBudget::default()
            )
            .is_err(),
            "{body}"
        );
    }
    let foreign = super::support::fixture();
    let foreign_store = crate::store::Store::empty(foreign.ir.clone());
    let lua = Lua::new();
    let mut codec = Codec::new(&lua).unwrap();
    let encoded = codec.encode_output(&captured).unwrap();
    let (value, provenance) = codec
        .decode_output(
            &foreign.ir,
            &foreign.ir.graph().fields()[foreign.changed.0].ty,
            &encoded,
        )
        .unwrap();
    assert!(
        crate::output::validate_provenance_detailed(
            &foreign_store.snapshot(),
            foreign.changed,
            &value,
            &provenance,
            &mut ValueBudget::default()
        )
        .is_err()
    );
}
