//! Test model, key, and field coverage plus declaration limits.

use std::fmt::Write as _;

use chigiri::core::Source;

use crate::ir::{FieldPlan, MockIr, Wrapper};

pub(super) const BLOG: &str = include_str!("../../../fixtures/blog.graphql");

pub(super) fn parse(source: &str) -> Result<MockIr, crate::schema::SchemaError> {
    MockIr::parse(vec![Source::new("schema.graphql", source)])
}

#[test]
fn rsm_02_blog_resolves_models_slots_exact_root_coverage_and_permissions() {
    let ir = parse(BLOG).unwrap();
    let user = ir.model_for(ir.graph().type_id("User").unwrap()).unwrap();
    let post = ir.model_for(ir.graph().type_id("Post").unwrap()).unwrap();
    assert_eq!(ir.models().len(), 2);
    assert_eq!(ir.models()[user.0].slots.len(), 2);
    assert_eq!(ir.models()[post.0].slots.len(), 4);
    assert!(!ir.models()[post.0].slots.contains_key("author"));
    let author = &ir.slots()[ir.models()[post.0].slots["authorId"].0];
    assert_eq!(ir.graph().types()[author.ty.named.0].name.as_ref(), "ID");
    assert_eq!(author.ty.wrappers.as_ref(), [Wrapper::NonNull]);
    assert_eq!(ir.scripts().len(), 6);
    assert_eq!(
        ir.scripts()
            .iter()
            .filter(|script| script.role.is_mutation_root())
            .count(),
        4
    );
    assert!(
        crate::luau::static_candidates(&ir)
            .unwrap()
            .iter()
            .all(|candidate| candidate.source.contains("NOT_IMPLEMENTED"))
    );
    for (index, field) in ir.graph().fields().iter().enumerate() {
        if ir.graph().types()[field.parent.0].kind == chigiri::core::schema::TypeKind::Object {
            assert!(ir.field_plan(crate::ir::FieldId(index)).is_some());
        }
    }
}

#[test]
fn rsm_02_optional_identity_plain_objects_and_invalid_roles_are_distinguished() {
    for fields in [
        "id:ID",
        "id:[ID!]!",
        "id:Int!",
        "id(x:Int):ID!",
        "other:ID!",
        "id:ID! @virtual",
    ] {
        assert!(
            parse(&format!("type Item{{{fields}}} type Query{{items:[Item]}}")).is_ok(),
            "{fields}"
        );
    }
    assert!(parse("type Query{x:Int}").unwrap().models().is_empty());
    for schema in [
        r#"type Item{id:String! @constraint(maxLength:0)} type Query{x:Int}"#,
        r#"type Item{id:String! @constraint(minLength:257)} type Query{x:Int}"#,
        r#"type Query{x:Int @relation}"#,
        r#"type Item{id:ID! peer:Item @relation(fields:["id"],references:["id"])} type Query{x:Int}"#,
        r#"type Item{id:ID! peer:Item @relation(fields:["peer"],references:["id"])} type Query{x:Int}"#,
        r#"type DTO{x(arg:Int):Int @virtual} type Query{x:DTO}"#,
    ] {
        let error = parse(schema).err().unwrap();
        assert!(error.span.is_some());
        assert!(!error.to_string().contains(schema));
    }
}

#[test]
fn rsm_07_manual_and_dto_fields_are_explicit_and_interface_roles_are_not_inherited() {
    let ir = parse(
        r#"
        interface Node { id:ID! label:String @virtual }
        type Item implements Node { id:ID! label:String }
        type Payload { item:Item text:String formatted(prefix:String):String }
        union Result = Item | Payload
        type Query { search:Result }
        type Mutation { reset:Payload }
    "#,
    )
    .unwrap();
    assert_eq!(ir.scripts().len(), 3);
    let item = &ir.graph().types()[ir.graph().type_id("Item").unwrap().0];
    assert!(matches!(
        ir.field_plan(item.fields["label"]),
        Some(FieldPlan::Stored(_))
    ));
    let dto = &ir.graph().types()[ir.graph().type_id("Payload").unwrap().0];
    assert!(matches!(
        ir.field_plan(dto.fields["item"]),
        Some(FieldPlan::Stored(_))
    ));
    assert_eq!(
        ir.scripts()
            .iter()
            .filter(|script| script.role.is_mutation_root())
            .count(),
        1
    );
    assert!(
        crate::luau::static_candidates(&ir)
            .unwrap()
            .iter()
            .all(|candidate| candidate.source.contains("NOT_IMPLEMENTED"))
    );
}

#[test]
fn rsm_16_model_and_script_coordinate_limits_are_inclusive() {
    let mut models = String::from("type Query{x:Int}");
    for index in 0..64 {
        write!(models, "type M{index}{{id:ID!}}").unwrap();
    }
    assert_eq!(parse(&models).unwrap().models().len(), 64);
    assert_eq!(
        parse(&format!("{models} type Extra{{id:ID!}}"))
            .err()
            .unwrap()
            .code,
        "model count exceeded"
    );
    let mut fields = String::new();
    for index in 0..256 {
        write!(fields, "f{index}:Int ").unwrap();
    }
    assert_eq!(
        parse(&format!("type Query{{{fields}}}"))
            .unwrap()
            .scripts()
            .len(),
        256
    );
    assert_eq!(
        parse(&format!("type Query{{{fields} extra:Int}}"))
            .err()
            .unwrap()
            .code,
        "script coordinate count exceeded"
    );
}

#[test]
fn sd_fixture_domains_have_explicit_relations_and_only_manual_script_candidates() {
    for (source, models, scripts) in [
        (
            include_str!("../../../fixtures/projects.graphql").to_owned(),
            4,
            20,
        ),
        (
            format!(
                "{}\n{}",
                include_str!("../../../fixtures/projects.graphql"),
                include_str!("../../../fixtures/model-api.graphql")
            ),
            4,
            29,
        ),
        (
            include_str!("../../../fixtures/hook_measure.graphql").to_owned(),
            1,
            2,
        ),
        (
            include_str!("../../../fixtures/inference-work.graphql").to_owned(),
            2,
            7,
        ),
    ] {
        let ir = parse(&source).unwrap();
        assert_eq!(ir.models().len(), models);
        assert_eq!(ir.scripts().len(), scripts);
        assert!(
            crate::luau::static_candidates(&ir)
                .unwrap()
                .iter()
                .all(|candidate| candidate.source.contains("NOT_IMPLEMENTED"))
        );
    }
}
