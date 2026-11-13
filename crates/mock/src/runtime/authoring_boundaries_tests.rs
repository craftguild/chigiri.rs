//! ADR-0047 stored-value projection and provenance acceptance.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::tests::{record, run, script};
use crate::ir::MockIr;
use crate::luau::Contract;
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;

fn contract(public: &str, storage: &str) -> Contract {
    Contract::new(Arc::new(
        MockIr::parse_with_storage(
            vec![Source::new("schema.graphql", public)],
            vec![Source::new("storage.graphql", storage)],
        )
        .unwrap(),
    ))
    .unwrap()
}

#[test]
fn ab_05_06_stored_projection_keeps_private_nested_parent_without_record_authority() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        interface Asset { url:String! }
        type Meta { label:String! }
        type Image implements Asset { url:String! nested:Meta! owner:String! @virtual }
        type Post { id:ID! image:Image! images:[Image!]! maybe:Image asset:Asset! }
        type Query { forged:Image! }
        type Mutation { make:Post! }
        "#,
        "extend type Image{ownerId:ID!} extend type Meta{secret:String!}",
    );
    let make = script(
        &contract,
        "Mutation.make",
        r#"local image={url='one',ownerId='u1',nested={label='meta',secret='inside'}}
        return ctx.models.Post:create({
            id='p',image=image,images={image},maybe=image,
            asset={__typename='Image' :: 'Image',url='asset',ownerId='u2',nested={label='abstract',secret='hidden'}},
        })"#,
    );
    let owner = script(
        &contract,
        "Image.owner",
        "return parent.ownerId .. ':' .. parent.nested.secret",
    );
    let mut store = Store::empty(contract.ir().clone());
    let post = record(run(&make, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(row) = post.data() else {
        panic!("post record required")
    };
    let ir = contract.ir();
    let post_type = ir.graph().type_id("Post").unwrap();
    let post_model = ir.model_for(post_type).unwrap();
    let image_field = ir.graph().types()[post_type.0].fields["image"];
    let image_slot = ir.models()[post_model.0].slots["image"];
    let image = post
        .snapshot()
        .reader()
        .stored_output(image_field, row.get(image_slot).unwrap())
        .unwrap();
    let OutputValue::Object(image) = image else {
        panic!("stored image required")
    };
    assert!(matches!(image.data(), ObjectData::Stored(_)));
    let owner = super::execute_nested(
        &owner,
        &Object::new(),
        &image,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(owner, OutputValue::Scalar(Value::String(value)) if value == "u1:inside"));

    for name in ["images", "maybe", "asset"] {
        let field = ir.graph().types()[post_type.0].fields[name];
        let slot = ir.models()[post_model.0].slots[name];
        assert!(
            post.snapshot()
                .reader()
                .stored_output(field, row.get(slot).unwrap())
                .is_ok(),
            "{name}"
        );
    }
    let image_model = ir.model_for(ir.graph().type_id("Image").unwrap()).unwrap();
    assert_eq!(
        store.snapshot().count(image_model, &Object::new()).unwrap(),
        0
    );
    assert!(
        post.snapshot()
            .reader()
            .output(image_field, row.get(image_slot).unwrap())
            .is_err()
    );

    let forged = script(
        &contract,
        "Query.forged",
        "return {url='x',ownerId='secret',nested={label='m',secret='private'}} :: any",
    );
    assert!(run(&forged, &Object::new(), &mut store).is_err());
    assert_eq!(store.snapshot().size().0, 1);
}
