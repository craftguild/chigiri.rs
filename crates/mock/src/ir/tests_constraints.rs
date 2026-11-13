//! Test scalar constraints and field/list constraint combinations.

use chigiri::core::Source;

use super::{Representation, SchemaGraph};

fn parse(source: &str) -> Result<SchemaGraph, crate::schema::SchemaError> {
    SchemaGraph::parse(vec![Source::new("schema", source)])
}

#[test]
fn rsm_04_all_representations_are_explicit_and_specified_by_is_only_metadata() {
    for (name, expected, constraints) in [
        (
            "STRING",
            Representation::String,
            ",minBytes:0,maxBytes:4096",
        ),
        ("BOOLEAN", Representation::Boolean, ""),
        ("INTEGER", Representation::Integer, ",min:-32,max:32"),
        ("NUMBER", Representation::Number, ",min:-1.5,max:2.5"),
        ("OBJECT", Representation::Object, ""),
        ("ARRAY", Representation::Array, ",minItems:0,maxItems:100"),
        ("JSON", Representation::Json, ""),
    ] {
        let graph = parse(&format!(
            "scalar Token @specifiedBy(url:\"https://invalid.invalid/codec\") @representation(type:{name}{constraints}) type Query{{value:Token}}"
        )).unwrap();
        let ty = &graph.types()[graph.type_id("Token").unwrap().0];
        assert_eq!(ty.scalar.as_ref().unwrap().representation, expected);
        assert_eq!(
            graph
                .source()
                .semantic()
                .type_by_name("Token")
                .unwrap()
                .directives()
                .len(),
            2
        );
    }
}

#[test]
fn rsm_04_inapplicable_contradictory_nonfinite_or_over_budget_constraints_fail() {
    for annotation in [
        "",
        "@representation(type:DATE)",
        "@representation(type:STRING,min:0)",
        "@representation(type:BOOLEAN,maxBytes:8)",
        "@representation(type:OBJECT,maxItems:8)",
        "@representation(type:JSON,maxBytes:8)",
        "@representation(type:NUMBER,min:4,max:3)",
        "@representation(type:NUMBER,min:1e999)",
        "@representation(type:STRING,minBytes:-1)",
        "@representation(type:STRING,maxBytes:4097)",
        "@representation(type:ARRAY,minItems:1001)",
        "@representation(type:ARRAY,maxItems:1001)",
        "@representation(type:ARRAY,minItems:2,maxItems:1)",
        "@representation(type:STRING,regex:\"secret\")",
    ] {
        let error = parse(&format!(
            "scalar Token {annotation} type Query{{value:Token}}"
        ))
        .err()
        .unwrap();
        assert!(error.span.is_some());
        assert!(!error.to_string().contains("secret"));
    }
    assert!(parse("scalar Int @representation(type:STRING) type Query{x:Int}").is_err());
}

#[test]
fn sd_12_field_conditions_distinguish_outer_list_and_scalar_leaves() {
    let graph = parse(
        r#"
        scalar Text @representation(type:STRING,maxBytes:32)
        input Patch { text: Text @constraint(maxLength:16) }
        type Query {
            items: [[Text]] @constraint(minItems:1,maxItems:10)
            label(input: Patch): Text @constraint(minLength:2,maxLength:8)
        }
    "#,
    )
    .unwrap();
    let query = &graph.types()[graph.query().0];
    let items = &graph.fields()[query.fields["items"].0];
    assert_eq!(items.constraints.max_items, Some(10));
    assert_eq!(items.constraints.max_length, None);
    assert_eq!(items.ty.list_depth(), 2);
    let text = &graph.types()[graph.type_id("Text").unwrap().0];
    assert_eq!(
        text.scalar.as_ref().unwrap().constraints.max_bytes,
        Some(32)
    );
    let patch = &graph.types()[graph.type_id("Patch").unwrap().0];
    assert_eq!(
        graph.inputs()[patch.inputs["text"].0]
            .constraints
            .max_length,
        Some(16)
    );
    for field in [
        "value: String @constraint(maxItems:3)",
        "value: Int @constraint(maxLength:3)",
    ] {
        assert!(parse(&format!("type Query{{{field}}}")).is_err());
    }
}

#[test]
fn rsm_04_scalar_and_field_constraint_intersections_must_be_satisfiable() {
    for (scalar, field) in [
        ("type:STRING,minBytes:16", "maxLength:3"),
        ("type:NUMBER,max:4", "min:5"),
        ("type:ARRAY,minItems:5", "maxItems:4"),
    ] {
        for body in [
            format!("type Query{{x:Token @constraint({field})}}"),
            format!("input Data{{x:Token @constraint({field})}} type Query{{x(input:Data):Int}}"),
            format!("type Query{{x(input:Token @constraint({field})):Int}}"),
        ] {
            assert!(parse(&format!("scalar Token @representation({scalar}) {body}")).is_err());
        }
    }
    assert!(parse("scalar Token @representation(type:STRING,minBytes:16) type Query{x:Token @constraint(maxLength:16)}").is_ok());
    assert!(parse("scalar Token @representation(type:ARRAY,minItems:5) type Query{x:[Token] @constraint(maxItems:4)}").is_ok());
}

#[test]
fn rsm_04_negative_zero_length_is_zero_but_negative_one_is_rejected() {
    for representation in ["STRING", "ARRAY"] {
        let (min, max) = if representation == "STRING" {
            ("minBytes", "maxBytes")
        } else {
            ("minItems", "maxItems")
        };
        for zero in ["0", "-0"] {
            let graph = parse(&format!("scalar Token @representation(type:{representation},{min}:{zero},{max}:{zero}) type Query{{x:Token}}" )).unwrap();
            let constraints = &graph.types()[graph.type_id("Token").unwrap().0]
                .scalar
                .as_ref()
                .unwrap()
                .constraints;
            assert_eq!(constraints.min_bytes.or(constraints.min_items), Some(0));
            assert_eq!(constraints.max_bytes.or(constraints.max_items), Some(0));
        }
        assert!(
            parse(&format!(
                "scalar Token @representation(type:{representation},{min}:-1) type Query{{x:Token}}"
            ))
            .is_err()
        );
    }
}

#[test]
fn rsm_04_integer_constraints_require_a_representable_value_in_the_intersection() {
    for source in [
        "scalar Token @representation(type:INTEGER,min:0.2,max:0.8) type Query{x:Token}",
        "type Query{x:Int @constraint(min:2147483648)}",
        "type Query{x:Int @constraint(max:-2147483649)}",
        "type Query{x:Int @constraint(min:-0.8,max:-0.2)}",
        "scalar Token @representation(type:INTEGER,min:0.1,max:1.9) type Query{x:Token @constraint(min:1.1,max:2.9)}",
        "scalar Token @representation(type:INTEGER,min:0.1,max:1.9) input I{x:Token @constraint(min:1.1,max:2.9)} type Query{x(input:I):Int}",
    ] {
        assert!(parse(source).is_err());
    }
    for source in [
        "scalar Token @representation(type:INTEGER,min:1e200,max:1e200) type Query{x:Token}",
        "type Query{x:Int @constraint(min:2147483647,max:2147483647)}",
        "type Query{x:Int @constraint(min:-2147483648,max:-2147483648)}",
        "scalar Token @representation(type:INTEGER,min:0.1,max:1.9) type Query{x:Token @constraint(min:1,max:2.9)}",
    ] {
        assert!(parse(source).is_ok());
    }
}
