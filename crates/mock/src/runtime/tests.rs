//! Exercise resolver context APIs in a real VM and validate return values plus every model exchange.

use super::*;
use crate::ir::{MockIr, ModelId, ScriptId};
use crate::luau::{Contract, ScriptCandidate};
use crate::output::ObjectData;
use chigiri::core::{Source, Value};

pub(super) fn contract(sdl: &str) -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new("runtime.graphql", sdl)]).unwrap(),
    ))
    .unwrap()
}

pub(super) fn script(contract: &Contract, coordinate: &str, body: &str) -> Script {
    let (parent, field) = coordinate.split_once('.').unwrap();
    let graph = contract.ir().graph();
    let field = graph.types()[graph.type_id(parent).unwrap().0].fields[field];
    let id = contract
        .ir()
        .scripts()
        .iter()
        .position(|script| script.field == field)
        .unwrap();
    contract
        .script(ScriptCandidate {
            script: ScriptId(id),
            source: crate::luau::render_module(contract.ir(), ScriptId(id), body).unwrap(),
        })
        .unwrap()
}

pub(super) fn args(pairs: &[(&str, Value)]) -> Object {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

pub(super) fn run(
    script: &Script,
    args: &Object,
    store: &mut Store,
) -> Result<OutputValue, ScriptError> {
    execute_root(
        script,
        args,
        store,
        &Cancellation::default(),
        &Cancellation::default(),
    )
}

pub(super) fn record(output: OutputValue) -> OutputObject {
    match output {
        OutputValue::Object(object) => object,
        _ => panic!("object output required"),
    }
}

pub(super) fn input(name: &str) -> Object {
    args(&[(
        "input",
        Value::Object(args(&[("name", Value::String(name.into()))])),
    )])
}

#[test]
fn sa_01_standard_capabilities_execute_with_the_published_surface() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let script = script(
        &contract,
        "Query.value",
        r#"
local values = {3, 1}
table.insert(values, 1, 4)
table.insert(values, 2)
table.sort(values)
local copied = table.clone(values)
assert(table.remove(copied) == 4)
local asserted, discarded = assert(1, "not returned")
assert(asserted == 1 and discarded == nil)
assert(table.concat(copied, ",") == "1,2,3")
assert(string.len("abc") == 3 and ("ABC"):lower() == "abc")
assert(string.upper("abc") == "ABC" and string.sub("abc", 2) == "bc")
assert(string.find("abc", "b", 1, true) == 2)
local dynamicFind: any = string.find
local first, last, capture = dynamicFind("abc", "(b)")
assert(first == 2 and last == 2 and capture == nil)
assert(math.abs(-2) == 2 and math.ceil(1.2) == 2 and math.floor(1.8) == 1)
assert(tostring(2) == "2" and tonumber("2") == 2)
return math.min(values[1], math.max(values[2], values[3]))
"#,
    );
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&script, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Int(1))
    ));
}

#[test]
fn sa_04_non_http_execution_exposes_null_request() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Boolean! }");
    let script = script(&contract, "Query.value", "return ctx.request == ctx.null");
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&script, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn rsm_05_09_manual_blog_operations_execute_in_real_vm_and_keep_old_snapshots() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"));
    let set: Vec<_> = [
        ("Query.getUsers", "return ctx.models.User:find({limit=args.limit})"),
        ("Query.findPost", r#"
            local filters: {ChigiriPostWhere} = {}
            local id = args.id
            local authorId = args.authorId
            if type(id) == "string" then
                local filter: ChigiriPostWhere = {field="id", op="eq", value=id}
                filters[#filters + 1] = filter
            end
            if type(authorId) == "string" then
                local filter: ChigiriPostWhere = {field="authorId", op="eq", value=authorId}
                filters[#filters + 1] = filter
            end
            return ctx.models.Post:find({where={all=filters},limit=args.limit})
        "#),
        ("Mutation.createUser", "return ctx.models.User:create({id=args.input.name,name=args.input.name})"),
        ("Mutation.createPost", "return ctx.models.Post:create({id='Post-1',title=args.input.title,body=args.input.body,authorId=args.input.authorId})"),
        ("Mutation.updatePost", "return ctx.models.Post:update({where={field='id',op='eq',value=args.id}},args.input)[1]"),
        ("Mutation.deletePost", "return ctx.models.Post:delete({where={field='id',op='eq',value=args.id}})>0"),
    ].into_iter().map(|(name, body)| script(&contract, name, body)).collect();
    let get = |name: &str| {
        set.iter()
            .find(|script| script.coordinate() == name)
            .unwrap()
    };
    let mut store = Store::empty(contract.ir().clone());
    for name in ["Ada", "Grace"] {
        run(get("Mutation.createUser"), &input(name), &mut store).unwrap();
    }
    let post = record(
        run(
            get("Mutation.createPost"),
            &args(&[(
                "input",
                Value::Object(args(&[
                    ("title", Value::String("first".into())),
                    ("body", Value::String("body".into())),
                    ("authorId", Value::String("Ada".into())),
                ])),
            )]),
            &mut store,
        )
        .unwrap(),
    );
    let changed = record(
        run(
            get("Mutation.updatePost"),
            &args(&[
                ("id", Value::String("Post-1".into())),
                (
                    "input",
                    Value::Object(args(&[("authorId", Value::String("Grace".into()))])),
                ),
            ]),
            &mut store,
        )
        .unwrap(),
    );
    let slot = contract.ir().models()[1].slots["authorId"];
    for (output, id) in [(&post, "Ada"), (&changed, "Grace")] {
        let ObjectData::Model(row) = output.data() else {
            panic!()
        };
        assert_eq!(row.get(slot), Some(&Value::String(id.into())));
    }
    let result = run(
        get("Query.findPost"),
        &args(&[("limit", Value::Int(20)), ("authorId", Value::Null)]),
        &mut store,
    )
    .unwrap();
    assert!(matches!(result, OutputValue::List(rows) if rows.len() == 1));
    for (id, author, expected) in [
        ("Post-1", "Grace", 1),
        ("Post-1", "Ada", 0),
        ("missing", "Grace", 0),
    ] {
        let result = run(
            get("Query.findPost"),
            &args(&[
                ("limit", 20.into()),
                ("id", id.into()),
                ("authorId", author.into()),
            ]),
            &mut store,
        )
        .unwrap();
        assert!(matches!(result, OutputValue::List(rows) if rows.len() == expected));
    }
    let removed = run(
        get("Mutation.deletePost"),
        &args(&[("id", Value::String("Post-1".into()))]),
        &mut store,
    )
    .unwrap();
    assert!(matches!(removed, OutputValue::Scalar(Value::Boolean(true))));
    assert_eq!(store.snapshot().size().0, 2);
    assert!(post.snapshot().record(ModelId(1), "Post-1").is_some());
}

#[test]
fn rsm_11_14_caught_host_error_and_bad_return_rollback_all_models_and_registration() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"));
    let mut store = Store::empty(contract.ir().clone());
    for body in [
        "local row = ctx.models.User:create({id = 'User-1', name = 'rolled back'})\npcall(function() ctx.models.Post:create({id = 'Post-1', title = 'x', body = 'x', authorId = 'missing'}) end)\nreturn row",
        "local row = ctx.models.User:create({id = 'User-1', name = 'rolled back'})\nlocal model: any = ctx.models.User\npcall(function() model:update({where={field='id',op='eq',value=row.id}}, {name = ctx.null}) end)\nreturn row",
        "local row = ctx.models.User:create({id = 'User-1', name = 'rolled back'})\nrow.name = 'forged'\nreturn row",
        "ctx.models.User:create({id = 'User-1', name = 'rolled back'})\nreturn {id = 'fake', name = 'fake'}",
        "ctx.models.User:create({id = 'User-1', name = 'rolled back'})\nerror('private payload')",
    ] {
        let script = script(&contract, "Mutation.createUser", body);
        let error = run(&script, &input("x"), &mut store).err().unwrap();
        assert!(!error.to_string().contains("private payload"));
        assert_eq!(store.snapshot().size().0, 0);
    }
    let script = script(
        &contract,
        "Mutation.createUser",
        "return ctx.models.User:create({id='User-1',name=args.input.name})",
    );
    let created = record(run(&script, &input("good"), &mut store).unwrap());
    let ObjectData::Model(row) = created.data() else {
        panic!()
    };
    assert_eq!(row.id().unwrap(), "User-1");
    assert_eq!(row.row(), 0);
}

#[test]
fn rsm_07_08_nested_manual_reads_parent_snapshot_and_rejects_dynamic_write() {
    let _test_case = crate::vm_gate::test_case();
    let source = r#"
        type Item { id: ID! name: String! payload: Payload! label: String! @virtual }
        type Payload { name: String! label: String! @virtual }
        type Query { items: [Item!]! }
        type Mutation { create: Item! }
    "#;
    let types = contract(source);
    let mut store = Store::empty(types.ir().clone());
    let create = script(
        &types,
        "Mutation.create",
        "return ctx.models.Item:create({id='Item-1',name='original',payload={name='dto'}})",
    );
    let parent = record(run(&create, &Object::new(), &mut store).unwrap());
    let change = script(
        &types,
        "Mutation.create",
        "return ctx.models.Item:update({where={field='id',op='eq',value='Item-1'}}, {name = 'later'})[1]",
    );
    run(&change, &Object::new(), &mut store).unwrap();
    let label = script(
        &types,
        "Item.label",
        "assert(parent.name == 'original') return ctx.models.Item:find({limit = 1})[1].name",
    );
    let result = execute_nested(
        &label,
        &Object::new(),
        &parent,
        &Cancellation::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(matches!(result, OutputValue::Scalar(Value::String(value)) if value == "original"));
    let bad = script(
        &types,
        "Item.label",
        "local model = ctx.models.Item :: any\nmodel:create({id='forbidden',name='forbidden',payload={name='dto'}})\nreturn parent.name",
    );
    assert!(
        execute_nested(
            &bad,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(store.snapshot().size().0, 1);
    let item = types
        .ir()
        .model_for(types.ir().graph().type_id("Item").unwrap())
        .unwrap();
    let field = types.ir().graph().types()[types.ir().models()[item.0].ty.0].fields["payload"];
    let ObjectData::Model(row) = parent.data() else {
        panic!("model record required")
    };
    let dto = record(
        parent
            .snapshot()
            .reader()
            .output(
                field,
                row.get(types.ir().models()[item.0].slots["payload"])
                    .unwrap(),
            )
            .unwrap(),
    );
    let label = script(&types, "Payload.label", "return parent.name");
    let result = execute_nested(
        &label,
        &Object::new(),
        &dto,
        &Cancellation::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(matches!(result, OutputValue::Scalar(Value::String(value)) if value == "dto"));
    assert!(
        run(
            &create,
            &Object::new(),
            &mut Store::empty(contract(source).ir().clone())
        )
        .is_err()
    );
}

#[test]
fn rsm_08_mutation_coordinate_is_read_only_when_invoked_on_nested_root_type() {
    let _test_case = crate::vm_gate::test_case();
    let types = contract(
        r#"
        type Item { id: ID! name: String! }
        type Query { payload: Mutation! }
        type Mutation { create: Item! }
    "#,
    );
    let mut store = Store::empty(types.ir().clone());
    let payload = script(&types, "Query.payload", "return {}");
    let parent = record(run(&payload, &Object::new(), &mut store).unwrap());
    let create = script(
        &types,
        "Mutation.create",
        "if ctx.scope == 'mutation' then return ctx.models.Item:create({id='Item-1',name='root only'}) else error('root only') end",
    );
    assert!(
        execute_nested(
            &create,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(store.snapshot().size().0, 0);
    run(&create, &Object::new(), &mut store).unwrap();
    assert_eq!(store.snapshot().size().0, 1);
    let parent_read = script(
        &types,
        "Mutation.create",
        "assert(parent ~= nil) assert(ctx.scope == 'read') return ctx.models.Item:find({limit = 1})[1]",
    );
    // The parent retains its pre-create snapshot, so nested resolution cannot switch to the new record.
    assert!(
        execute_nested(
            &parent_read,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default()
        )
        .is_err()
    );
}
