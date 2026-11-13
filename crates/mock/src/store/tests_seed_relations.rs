//! Verify seed-relation diagnostics identify model, line, and field without exposing identity values.

use std::sync::Arc;

use chigiri::core::Source;

use super::SeedData;
use crate::ir::{MockIr, ModelId};

#[test]
fn mp_19_required_inverse_diagnostic_names_the_field_imposing_the_minimum() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "schema/001.graphql",
            r#"
        type User {
            id:ID!
            optional:Profile @relation(name:"optional")
            required:Profile! @relation
        }
        type Profile {
            id:ID! owner:User! @relation(fields:["ownerId"],references:["id"])
            optionalOwner:User @relation(fields:["optionalId"],references:["id"],name:"optional")
        }
        type Query {users:[User!]!}
    "#,
        )])
        .unwrap(),
    );
    let error = SeedData::jsonl(
        &ir,
        &[(ModelId(0), "{\"id\":\"private\"}"), (ModelId(1), "")],
    )
    .unwrap()
    .store(ir.clone())
    .err()
    .unwrap();
    assert_eq!(error.model, Some(ModelId(0)));
    assert_eq!(error.line, 1);
    assert_eq!(error.field.as_deref(), Some("required"));
    assert_eq!(error.message, "required inverse missing");
}

#[test]
fn mp_19_jsonl_relations_fail_after_all_models_with_redacted_locations() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "schema/001.graphql",
            r#"
        type User { id:ID! profile:Profile! @relation }
        type Profile { id:ID! owner:User! @relation(fields:["ownerId"],references:["id"]) }
        type Query { users:[User!]! }
    "#,
        )])
        .unwrap(),
    );
    for (users, profiles, model, line, field, reason) in [
        (
            "{\"id\":\"u\"}",
            "{\"id\":\"p\",\"ownerId\":\"private missing\"}",
            1,
            1,
            "owner",
            "unresolved relation reference",
        ),
        (
            "{\"id\":\"u\"}\n{\"id\":\"private missing\"}",
            "{\"id\":\"p\",\"ownerId\":\"u\"}",
            0,
            2,
            "profile",
            "required inverse missing",
        ),
        (
            "{\"id\":\"u\"}",
            "{\"id\":\"p\",\"ownerId\":\"u\"}\n{\"id\":\"q\",\"ownerId\":\"u\"}",
            1,
            1,
            "owner",
            "inverse cardinality exceeded",
        ),
    ] {
        for files in [
            [(ModelId(0), users), (ModelId(1), profiles)],
            [(ModelId(1), profiles), (ModelId(0), users)],
        ] {
            let error = SeedData::jsonl(&ir, &files)
                .unwrap()
                .store(ir.clone())
                .err()
                .unwrap();
            assert_eq!(error.model, Some(ModelId(model)));
            assert_eq!(error.line, line);
            assert_eq!(error.field.as_deref(), Some(field));
            assert_eq!(error.message, reason);
            assert!(!format!("{error:?}").contains("private"));
        }
    }
}

#[test]
fn mp_17_31_jsonl_total_record_and_node_budgets_do_not_reset_between_files() {
    let ir = MockIr::parse(vec![Source::new(
        "schema/001.graphql",
        r#"
        scalar Data @representation(type:JSON)
        type A { id:ID! data:Data }
        type B { id:ID! data:Data }
        type C { id:ID! }
        type Query { count:Int! }
    "#,
    )])
    .unwrap();
    let records = (0..1000)
        .map(|id| format!("{{\"id\":\"{id}\"}}\n"))
        .collect::<String>();
    SeedData::jsonl(
        &ir,
        &[
            (ModelId(0), &records),
            (ModelId(1), &records),
            (ModelId(2), ""),
        ],
    )
    .unwrap();
    let error = SeedData::jsonl(
        &ir,
        &[
            (ModelId(0), &records),
            (ModelId(1), &records),
            (ModelId(2), "{\"id\":\"extra\"}"),
        ],
    )
    .err()
    .unwrap();
    assert_eq!(error.model, Some(ModelId(2)));
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "seed record capacity");
    let values = format!("[{}]", vec!["0"; 100].join(","));
    let payload = format!("[{}]", vec![values; 100].join(","));
    let row = format!("{{\"id\":\"row\",\"data\":{payload}}}\n");
    let nine = row.repeat(9);
    assert!(nine.len() + row.len() < crate::limits::SEED_BYTES);
    SeedData::jsonl(
        &ir,
        &[(ModelId(0), &nine), (ModelId(1), ""), (ModelId(2), "")],
    )
    .unwrap();
    let error = SeedData::jsonl(
        &ir,
        &[(ModelId(0), &nine), (ModelId(1), &row), (ModelId(2), "")],
    )
    .err()
    .unwrap();
    assert_eq!(error.model, Some(ModelId(1)));
    assert_eq!(error.line, 1);
}
