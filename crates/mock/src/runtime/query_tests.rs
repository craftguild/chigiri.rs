//! Extended-query coverage through real Luau, including shapes, limits, and rollback.

use super::tests::{contract, run, script};
use crate::output::OutputValue;
use crate::store::Store;
use chigiri::core::{Object, Value};

const SDL: &str = r#"
type Item { id:ID! name:String! rank:Int! note:String }
type Query { inspect:Boolean! }
type Mutation { edit:Boolean! }
"#;

#[test]
fn mr_03_06_08_09_real_vm_uses_predicate_sort_page_and_exact_count_above_100() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let populate = script(
        &contract,
        "Mutation.edit",
        r#"
        for i = 1, 130 do
            ctx.models.Item:create({id = 'Item-' .. i, name = 'alphabet', rank = i})
        end
        assert(ctx.models.Item:count({}) == 130)
        assert(ctx.models.Item:count({where = {field = 'rank', op = 'gt', value = 100}}) == 30)
        return #ctx.models.Item:find({}) == 130
    "#,
    );
    let populated = run(&populate, &Object::new(), &mut store).unwrap();
    assert!(matches!(
        populated,
        OutputValue::Scalar(Value::Boolean(true))
    ));
    let inspect = script(
        &contract,
        "Query.inspect",
        r#"
        local model = ctx.models.Item
        assert(model:count({where = ctx.null}) == 130)
        assert(model:count({where = {all = {}}}) == 130)
        assert(model:count({where = {any = {}}}) == 0)
        assert(model:count({where = {field = 'rank', op = 'in', value = {}}}) == 0)
        assert(model:count({where = {field = 'note', op = 'ne', value = 'x'}}) == 0)
        local rows = model:find({
            where = {all = {
                {field = 'name', op = 'contains', value = 'pha'},
                {any = {
                    {field = 'rank', op = 'in', value = {1, 1, 2}},
                    {field = 'rank', op = 'gt', value = 128},
                }},
            }},
            orderBy = {{field = 'rank', direction = 'desc'}},
            offset = 1, limit = 2,
        })
        assert(#rows == 2 and rows[1].rank == 129 and rows[2].rank == 2)
        rows[1].name = 'local copy'
        return model:count({where = {field = 'name', op = 'eq', value = 'alphabet'}}) == 130
    "#,
    );
    assert!(matches!(
        run(&inspect, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn mr_04_07_14_15_real_vm_rejects_holes_cycles_null_and_bad_query_calls_stickily() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    for code in [
        "m:find({where = {}})",
        "m:find({where = {name = 'legacy'}})",
        "m:count({where = {field = 'name', op = 'eq'}})",
        "m:count({where = {field = 'name', op = 'eq', value = ctx.null}})",
        "m:count({where = {field = 'rank', op = 'in', value = {[1] = 1, [3] = 2}}})",
        "m:count({where = {all = {[1] = {all = {}}, [3] = {all = {}}}}})",
        "local p = {} p.all = {p} m:count({where = p})",
        "m:find({orderBy = {[2] = {field = 'name', direction = 'asc'}}})",
        "m:find({orderBy = ctx.null})",
        "m:find({offset = ctx.null})",
        "m:find({limit = ctx.null})",
        "m:count({limit = 1})",
        "m:count()",
        "m:count({}, {})",
        "m.count({}, {})",
        "m:count({where = {field = 'id', op = 'contains', value = 'Item'}})",
        "m:count({where = {field = 'name', op = 'eq', value = 1}})",
        "m:find({limit = 0, where = {any = {({all = {}} :: any), {field = 'missing', op = 'eq', value = 1}}}})",
    ] {
        let body = format!(
            "local m = ctx.models.Item :: any\nm:create({{id = 'Item-1', name = 'rollback', rank = 1}})\npcall(function() {code} end)\nreturn true"
        );
        let candidate = script(&contract, "Mutation.edit", &body);
        assert!(
            run(&candidate, &Object::new(), &mut store).is_err(),
            "{code}"
        );
        assert_eq!(store.snapshot().size().0, 0, "{code}");
    }
    let next = script(
        &contract,
        "Mutation.edit",
        "local row = ctx.models.Item:create({id = 'Item-1', name = 'after', rank = 1}) return row.id == 'Item-1'",
    );
    assert!(matches!(
        run(&next, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}
