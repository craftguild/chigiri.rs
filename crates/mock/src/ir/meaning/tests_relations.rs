//! Verifies relation pairing, shared foreign keys, composite keys, cardinality,
//! and invalid declarations.

use super::tests::parse;
use crate::ir::{FieldPlan, RelationStorage, Wrapper};

const BLOG: &str = r#"
    type User { id: ID! posts: [Post!]! @relation }
    type Post { id: ID! author: User! @relation(fields: ["authorId"], references: ["id"]) }
    type Query { posts: [Post!]! }
"#;

#[test]
fn sd_relation_only_marked_fields_participate_and_owner_can_be_unidirectional() {
    let ir = parse(BLOG).unwrap();
    assert_eq!(ir.relations().len(), 1);
    let relation = &ir.relations()[0];
    assert_eq!(relation.inverses.len(), 1);
    assert!(matches!(
        ir.field_plan(relation.inverses[0]),
        Some(FieldPlan::Inverse { .. })
    ));
    let RelationStorage::ForeignKey { slots, key } = &relation.storage else {
        panic!("foreign-key storage required")
    };
    assert_eq!(slots.len(), 1);
    assert_eq!(ir.slots()[slots[0].0].name.as_ref(), "authorId");
    assert_eq!(ir.keys()[key.0].slots.len(), 1);
    let ir = parse(&BLOG.replace("posts: [Post!]! @relation", "posts: [Post!]!")).unwrap();
    assert!(ir.relations()[0].inverses.is_empty());
    let user = ir.graph().type_id("User").unwrap();
    let posts = ir.graph().types()[user.0].fields["posts"];
    assert!(matches!(ir.field_plan(posts), Some(FieldPlan::Stored(_))));
    assert!(
        parse(&BLOG.replace(
            "author: User! @relation(fields: [\"authorId\"], references: [\"id\"])",
            "author: User!"
        ))
        .is_err()
    );
}

#[test]
fn sd_relation_single_many_self_and_nullable_items_preserve_storage_shape() {
    let ir = parse(
        r#"
        type Member {
            code: String!
            partner: Member @relation(fields:["partnerCode"], references:["code"], name:"Pair")
            spouse: Member @relation(name:"Pair")
            teams: [Team]! @relation(fields:["teamCodes"], references:["code"])
        }
        type Team { code: Int! members: [Member!]! @relation }
        type Query { members: [Member!]! }
    "#,
    )
    .unwrap();
    assert_eq!(ir.relations().len(), 2);
    let self_edge = &ir.relations()[0];
    assert_eq!(self_edge.owner, self_edge.target);
    assert!(self_edge.unique);
    let RelationStorage::ForeignKey { slots, .. } = &self_edge.storage else {
        panic!("foreign-key storage required")
    };
    assert!(ir.slots()[slots[0].0].ty.nullable());
    let RelationStorage::ForeignKey { slots, .. } = &ir.relations()[1].storage else {
        panic!("foreign-key storage required")
    };
    assert_eq!(
        ir.slots()[slots[0].0].ty.wrappers.as_ref(),
        [Wrapper::NonNull, Wrapper::List]
    );
    assert_eq!(
        ir.graph().types()[ir.slots()[slots[0].0].ty.named.0]
            .name
            .as_ref(),
        "Int"
    );
}

#[test]
fn sd_relation_composite_public_hidden_and_key_immutability_share_slots() {
    let source = r#"
        type User { tenant: ID! code: Int! }
        type Post {
            tenantFK: ID
            author: User @relation(fields:["tenantFK", "codeFK"], references:["tenant", "code"])
        }
        type Query { posts: [Post!]! }
    "#;
    let ir = parse(source).unwrap();
    let relation = &ir.relations()[0];
    let RelationStorage::ForeignKey { slots, key } = &relation.storage else {
        panic!("composite foreign-key storage required")
    };
    assert_eq!(slots.len(), 2);
    let owner = &ir.models()[relation.owner.0];
    assert_eq!(owner.slots.len(), 2);
    assert_eq!(owner.slots["tenantFK"], slots[0]);
    assert_eq!(ir.keys()[key.0].slots.len(), 2);
    assert_eq!(ir.models()[relation.target.0].immutable_slots.len(), 2);
    for bad in [
        source.replace("tenantFK: ID", "tenantFK: String"),
        source.replace("tenantFK: ID", "tenantFK: ID!"),
        source.replace("code: Int!", "code: Int"),
        source.replace("code: Int!", "code: Float!"),
        source.replace("code: Int!", "code: Int! @virtual"),
        source.replace("[\"tenantFK\", \"codeFK\"]", "[\"tenantFK\", \"tenantFK\"]"),
        source.replace("[\"tenant\", \"code\"]", "[\"tenant\"]"),
    ] {
        assert!(parse(&bad).is_err(), "{bad}");
    }
}

#[test]
fn sd_relation_implicit_requires_both_marked_lists_and_nonnull_identity() {
    let source = "type A { id: Int! bs: [B!]! @relation } type B { id: Boolean! as: [A!]! @relation } type Query { a: A }";
    let ir = parse(source).unwrap();
    assert!(matches!(
        ir.relations()[0].storage,
        RelationStorage::Implicit { .. }
    ));
    assert_eq!(
        ir.models()
            .iter()
            .map(|model| model.slots.len())
            .sum::<usize>(),
        2
    );
    for bad in [
        source.replacen("@relation", "", 1),
        source.replace("id: Int!", "id: Int"),
        source.replace("id: Int!", "key: Int!"),
        source.replacen("@relation", "@relation(name:\"OneSide\")", 1),
        source.replace("bs: [B!]!", "bs: B"),
    ] {
        assert!(parse(&bad).is_err(), "{bad}");
    }
}

#[test]
fn sd_relation_ambiguity_names_arguments_and_invalid_keys_are_rejected() {
    let ambiguous = r#"
        type User { id: ID! posts: [Post!]! @relation }
        type Post {
            author: User @relation(fields:["authorId"], references:["id"])
            recipient: User @relation(fields:["recipientId"], references:["id"])
        }
        type Query { users: [User!]! }
    "#;
    assert!(parse(ambiguous).is_err());
    let named = ambiguous
        .replace(
            "posts: [Post!]! @relation",
            "posts: [Post!]! @relation(name:\"Author\")",
        )
        .replace(
            "fields:[\"authorId\"]",
            "name:\"Author\", fields:[\"authorId\"]",
        );
    assert_eq!(parse(&named).unwrap().relations().len(), 2);
    for bad in [
        BLOG.replace("author: User!", "author(extra:Int): User!"),
        BLOG.replace(
            "posts: [Post!]! @relation",
            "posts(limit:Int): [Post!]! @relation",
        ),
        BLOG.replace("author: User!", "author: [[User!]]!"),
        BLOG.replace("[\"authorId\"]", "[\"__typename\"]"),
        BLOG.replace("[\"authorId\"]", "[]"),
        BLOG.replace(", references: [\"id\"]", ""),
        BLOG.replace("references: [\"id\"]", "references: [\"missing\"]"),
        BLOG.replace(
            "author: User! @relation",
            "author: User! @virtual @relation",
        ),
    ] {
        assert!(parse(&bad).is_err(), "{bad}");
    }
}

#[test]
fn sd_relation_inverse_cardinality_uses_declared_constraints_without_paging() {
    let ir = parse(&BLOG.replace(
        "posts: [Post!]! @relation",
        "posts: [Post!]! @relation @constraint(minItems:2, maxItems:2)",
    ))
    .unwrap();
    assert_eq!(ir.relations()[0].min_refs, 2);
    assert_eq!(ir.relations()[0].max_refs, Some(2));
    let ir = parse(&BLOG.replace("posts: [Post!]! @relation", "post: Post! @relation")).unwrap();
    assert!(ir.relations()[0].unique);
    assert_eq!(ir.relations()[0].min_refs, 1);
    assert_eq!(ir.relations()[0].max_refs, Some(1));
}

#[test]
fn sd_relation_reference_key_rejects_known_empty_string_domains_at_schema_build() {
    let source = r#"
        type Key { code: String! BOUNDS }
        type Item { key: Key @relation(fields:["keyCode"],references:["code"]) }
        type Query { keys: [Key!]! }
    "#;
    for bounds in [
        "@constraint(minLength:257)".to_owned(),
        "@constraint(maxLength:0)".to_owned(),
        format!("@constraint(startsWith:\"{}\")", "€".repeat(86)),
        format!("@constraint(endsWith:\"{}\")", "x".repeat(257)),
        format!("@constraint(contains:\"{}\")", "x".repeat(257)),
    ] {
        assert!(
            parse(&source.replace("BOUNDS", &bounds)).is_err(),
            "{bounds}"
        );
    }
    for bounds in [
        "@constraint(minLength:256,maxLength:300)".to_owned(),
        format!("@constraint(startsWith:\"{}x\")", "€".repeat(85)),
        "@constraint(pattern:\"^[A-Z]+$\")".to_owned(),
    ] {
        assert!(
            parse(&source.replace("BOUNDS", &bounds)).is_ok(),
            "{bounds}"
        );
    }
    // Apply the same 256-byte domain to identity fields. Constraints on the
    // generated character set must not cause a static rejection.
    let source = source
        .replace("code: String!", "id: String!")
        .replace("[\"code\"]", "[\"id\"]");
    assert!(
        parse(&source.replace(
            "BOUNDS",
            &format!("@constraint(contains:\"{}\")", "The sun.".repeat(86))
        ))
        .is_err()
    );
    assert!(parse(&source.replace("BOUNDS", "@constraint(pattern:\"^[A-Z]+$\")")).is_ok());
}
