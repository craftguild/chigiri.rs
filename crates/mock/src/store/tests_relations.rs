//! Test two-phase seed validation, relation reconnection, required cycles, and `RESTRICT` behavior.

use crate::store::test_selection::parent as parent_record;

use crate::store::test_selection::id as select;

use chigiri::core::Value;

use super::tests::{BLOG, field, ir, model, object, row};
use super::{CallScope, Related, Store};

#[test]
fn rsm_10_seed_model_order_is_irrelevant_and_invalid_rows_or_references_fail() {
    let ir = ir(BLOG);
    let user = Value::List(vec![row(&[("id", "u".into()), ("name", "A".into())])]);
    let post = Value::List(vec![row(&[
        ("id", "p".into()),
        ("title", "T".into()),
        ("body", "B".into()),
        ("authorId", "u".into()),
    ])]);
    for seed in [
        object(&[("Post", post.clone()), ("User", user.clone())]),
        object(&[("User", user.clone()), ("Post", post.clone())]),
    ] {
        let store = Store::seeded(ir.clone(), &seed).unwrap();
        let snapshot = store.snapshot();
        let Related::One(author) = snapshot
            .related(
                field(&ir, "Post", "author"),
                &parent_record(&snapshot, field(&ir, "Post", "author"), "p"),
            )
            .unwrap()
        else {
            panic!("singular relation required")
        };
        assert_eq!(author.id().unwrap(), "u");
        let Related::Many(posts) = snapshot
            .related(
                field(&ir, "User", "posts"),
                &parent_record(&snapshot, field(&ir, "User", "posts"), "u"),
            )
            .unwrap()
        else {
            panic!("List of relationships")
        };
        assert_eq!(posts[0].as_ref().unwrap().id().unwrap(), "p");
    }
    for seed in [
        object(&[("Post", post)]),
        object(&[("Unknown", Value::List(vec![]))]),
        object(&[("User", Value::Null)]),
        object(&[("User", Value::List(vec![Value::Null]))]),
        object(&[("User", Value::List(vec![row(&[("name", "A".into())])]))]),
        object(&[(
            "User",
            Value::List(vec![row(&[
                ("id", "u".into()),
                ("name", "A".into()),
                ("unknown", Value::Null),
            ])]),
        )]),
        object(&[(
            "User",
            Value::List(vec![row(&[("id", "u".into()), ("name", "A".into())]); 2]),
        )]),
    ] {
        assert!(Store::seeded(ir.clone(), &seed).is_err());
    }
}

const NETWORK: &str = r#"
type Person {
  id:ID! partner:Person @relation(fields:["partnerId"],references:["id"])
  paired:Person @relation
  groups:[Group]! @relation(fields:["groupIds"],references:["id"],name:"groups")
}
type Group { id:ID! members:[Person!]! @relation(name:"groups") }
type Query { x:Int }
"#;

#[test]
fn rsm_11_many_to_many_nullable_self_reconnect_and_restrict_share_one_index() {
    let ir = ir(NETWORK);
    let (person, group) = (model(&ir, "Person"), model(&ir, "Group"));
    let mut store = Store::empty(ir.clone());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(group, &object(&[("id", "Group-1".into())]))
        .unwrap();
    tx.create(group, &object(&[("id", "Group-2".into())]))
        .unwrap();
    let groups = Value::List(vec!["Group-2".into(), Value::Null, "Group-1".into()]);
    tx.create(
        person,
        &object(&[
            ("id", "Person-1".into()),
            ("partnerId", "Person-1".into()),
            ("groupIds", groups),
        ]),
    )
    .unwrap();
    tx.create(
        person,
        &object(&[
            ("id", "Person-2".into()),
            ("groupIds", Value::List(vec!["Group-1".into()])),
        ]),
    )
    .unwrap();
    let snapshot = store.commit(tx.prepare().unwrap()).unwrap();
    let Related::Many(groups) = snapshot
        .related(
            field(&ir, "Person", "groups"),
            &parent_record(&snapshot, field(&ir, "Person", "groups"), "Person-1"),
        )
        .unwrap()
    else {
        panic!("list")
    };
    assert_eq!(
        groups
            .iter()
            .map(|record| record.as_ref().map(|r| r.id().unwrap()))
            .collect::<Vec<_>>(),
        [Some("Group-2"), None, Some("Group-1")]
    );
    let Related::Many(members) = snapshot
        .related(
            field(&ir, "Group", "members"),
            &parent_record(&snapshot, field(&ir, "Group", "members"), "Group-1"),
        )
        .unwrap()
    else {
        panic!("list")
    };
    assert_eq!(
        members
            .iter()
            .map(|record| record.as_ref().unwrap().id().unwrap())
            .collect::<Vec<_>>(),
        ["Person-1", "Person-2"]
    );
    let mut denied = store.begin(CallScope::MutationRoot).unwrap();
    assert!(denied.delete(group, &select("Group-1")).is_err());
    assert!(denied.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(
        person,
        &select("Person-2"),
        &object(&[("partnerId", "Person-1".into())]),
    )
    .unwrap();
    tx.update(
        person,
        &select("Person-1"),
        &object(&[("partnerId", Value::Null)]),
    )
    .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    let Related::One(paired) = store
        .snapshot()
        .related(
            field(&ir, "Person", "paired"),
            &parent_record(
                &store.snapshot(),
                field(&ir, "Person", "paired"),
                "Person-1",
            ),
        )
        .unwrap()
    else {
        panic!("singular relation required")
    };
    assert_eq!(paired.id().unwrap(), "Person-2");
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(
        person,
        &select("Person-2"),
        &object(&[("partnerId", Value::Null)]),
    )
    .unwrap();
    assert_eq!(tx.delete(person, &select("Person-1")).unwrap(), 1);
    tx.update(
        person,
        &select("Person-2"),
        &object(&[("groupIds", Value::List(vec![]))]),
    )
    .unwrap();
    assert_eq!(tx.delete(group, &select("Group-1")).unwrap(), 1);
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(snapshot.size().0, 4);
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn rsm_11_required_cycles_can_be_completed_before_commit_but_not_read_unresolved() {
    let source = r#"
      type Left { id:ID! right:Right! @relation(fields:["rightId"],references:["id"]) }
      type Right { id:ID! left:Left! @relation(fields:["leftId"],references:["id"]) }
      type Query { x:Int }
    "#;
    let ir = ir(source);
    let (left, right) = (model(&ir, "Left"), model(&ir, "Right"));
    let mut store = Store::empty(ir.clone());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        left,
        &object(&[("id", "Left-1".into()), ("rightId", "pending".into())]),
    )
    .unwrap();
    tx.create(
        right,
        &object(&[("id", "Right-1".into()), ("leftId", "Left-1".into())]),
    )
    .unwrap();
    tx.update(
        left,
        &select("Left-1"),
        &object(&[("rightId", "Right-1".into())]),
    )
    .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        left,
        &object(&[("id", "Left-2".into()), ("rightId", "pending".into())]),
    )
    .unwrap();
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        left,
        &object(&[("id", "Left-2".into()), ("rightId", "pending".into())]),
    )
    .unwrap();
    assert!(
        tx.related(
            field(&ir, "Left", "right"),
            &tx.state.record(left, "Left-2").unwrap().clone()
        )
        .is_err()
    );
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn rsm_10_duplicate_edges_and_inverse_uniqueness_or_required_cardinality_fail() {
    let ir = ir(NETWORK);
    let (person, group) = (model(&ir, "Person"), model(&ir, "Group"));
    let store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(group, &object(&[("id", "Group-1".into())]))
        .unwrap();
    assert!(
        tx.create(
            person,
            &object(&[
                ("id", "Person-1".into()),
                (
                    "groupIds",
                    Value::List(vec!["Group-1".into(), "Group-1".into()])
                ),
            ]),
        )
        .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    for id in ["Person-1", "Person-2"] {
        tx.create(
            person,
            &object(&[
                ("id", id.into()),
                ("partnerId", "Person-1".into()),
                ("groupIds", Value::List(vec![])),
            ]),
        )
        .unwrap();
    }
    assert!(tx.prepare().is_err());
    let ir = super::tests::ir(&NETWORK.replace("paired:Person", "paired:Person!"));
    let person = model(&ir, "Person");
    let store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        person,
        &object(&[("id", "Person-2".into()), ("groupIds", Value::List(vec![]))]),
    )
    .unwrap();
    assert!(tx.prepare().is_err());
}

#[test]
fn rsm_11_nullable_owner_list_and_self_only_delete_preserve_null_and_empty() {
    let ir = ir(&NETWORK.replace("groups:[Group]!", "groups:[Group]"));
    let person = model(&ir, "Person");
    let groups = field(&ir, "Person", "groups");
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        person,
        &object(&[
            ("id", "Person-1".into()),
            ("partnerId", "Person-1".into()),
            ("groupIds", Value::Null),
        ]),
    )
    .unwrap();
    tx.create(
        person,
        &object(&[("id", "Person-2".into()), ("groupIds", Value::List(vec![]))]),
    )
    .unwrap();
    let snapshot = store.commit(tx.prepare().unwrap()).unwrap();
    assert!(matches!(
        snapshot
            .related(groups, &parent_record(&snapshot, groups, "Person-1"))
            .unwrap(),
        Related::Null
    ));
    assert!(
        matches!(snapshot.related(groups, &parent_record(&snapshot, groups, "Person-2")).unwrap(), Related::Many(rows) if rows.is_empty())
    );
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.delete(person, &select("Person-1")).unwrap(), 1);
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().size().0, 1);
}

#[test]
fn rsm_16_inverse_returns_more_than_100_edges_and_preserves_old_snapshot() {
    let ir = ir(NETWORK);
    let (person, group) = (model(&ir, "Person"), model(&ir, "Group"));
    let members = field(&ir, "Group", "members");
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(group, &object(&[("id", "Group-1".into())]))
        .unwrap();
    for id in 0..100 {
        tx.create(
            person,
            &object(&[
                ("id", format!("Person-{id}").into()),
                ("groupIds", Value::List(vec!["Group-1".into()])),
            ]),
        )
        .unwrap();
    }
    let first = store.commit(tx.prepare().unwrap()).unwrap();
    assert!(
        matches!(first.related(members, &parent_record(&first, members, "Group-1")).unwrap(), Related::Many(rows) if rows.len() == 100)
    );
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(
        person,
        &object(&[
            ("id", "Person-100".into()),
            ("groupIds", Value::List(vec!["Group-1".into()])),
        ]),
    )
    .unwrap();
    let second = store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(second.size().0, 102);
    assert!(matches!(
        second.related(members, &parent_record(&second, members, "Group-1")).unwrap(),
        Related::Many(rows) if rows.len() == 101
    ));
    assert!(
        matches!(first.related(members, &parent_record(&first, members, "Group-1")).unwrap(), Related::Many(rows) if rows.len() == 100)
    );
}
