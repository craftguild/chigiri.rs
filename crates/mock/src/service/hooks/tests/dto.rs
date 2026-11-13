//! Observe concrete and abstract DTO values, stored shadows, and scripted fields at real invocation boundaries.

use super::*;
use HookSlot::*;
use serde_json::json;

#[tokio::test]
async fn hk_09_11_dto_union_interface_hooks_preserve_parent_and_preselection_outcome() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        r#"
        interface Node { id: ID! }
        type Packet implements Node {
            id: ID!
            child: Packet
            label(prefix: String! = "dto:"): String!
        }
        union Result = Packet
        type Query {
            packet: Result!
            node: Node!
        }
        "#,
        r#"{"Packet":[{"id":"root","child":{"id":"child"}}]}"#,
        &[
            ("Query.packet", "return ctx.models.Packet:find({})[1]"),
            ("Query.node", "return ctx.models.Packet:find({})[1]"),
            ("Packet.label", "return args.prefix .. parent.id"),
        ],
        &[
            (RequestBefore, "ctx.log('info','request',{})"),
            (OperationBefore, "ctx.log('info','operation',{})"),
            (FieldBefore, "ctx.log('info','before',{field=ctx.field})"),
            (
                FieldAfter,
                "ctx.log('info','after',{field=ctx.field,result=ctx.result})",
            ),
        ],
    );
    let (response, status) = fixture
        .query(
            r#"query($show:Boolean! = true) {
                throughUnion:packet { __typename ...Fields }
                throughInterface:node { __typename ...Fields }
                __schema { queryType { name } }
            }
            fragment Fields on Packet {
                id ... on Packet { id }
                name:label(prefix:"view:") @include(if:$show)
                child { id label }
                skipped:label @skip(if:true)
                excluded:child @include(if:false) { label }
            }"#,
        )
        .await;
    assert_eq!(status, None);
    let selected = json!({
        "__typename":"Packet","id":"root","name":"view:root",
        "child":{"id":"child","label":"dto:child"}
    });
    assert_eq!(
        response,
        json!({"data":{
            "throughUnion":selected,"throughInterface":selected,
            "__schema":{"queryType":{"name":"Query"}}
        }})
    );
    let logs = fixture.logs();
    assert_eq!(logs[0].0, RequestBefore);
    assert_eq!(logs[1].0, OperationBefore);
    assert_eq!(logs.len(), 26);
    let raw_child = json!({"__typename":"Packet","id":"child"});
    // The root view preserves its stored typename; the embedded child gains one only when projected independently.
    let raw_parent = json!({"__typename":"Packet","id":"root","child":{"id":"child"}});
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap()
            .size()
            .0,
        1
    );
    for (alias, root) in [
        ("throughUnion", "Query.packet"),
        ("throughInterface", "Query.node"),
    ] {
        for (suffix, coordinate, parent, args, value) in [
            (vec![], root, json!(null), json!({}), raw_parent.clone()),
            (
                vec!["id"],
                "Packet.id",
                raw_parent.clone(),
                json!({}),
                json!("root"),
            ),
            (
                vec!["name"],
                "Packet.label",
                raw_parent.clone(),
                json!({"prefix":"view:"}),
                json!("view:root"),
            ),
            (
                vec!["child"],
                "Packet.child",
                raw_parent.clone(),
                json!({}),
                raw_child.clone(),
            ),
            (
                vec!["child", "id"],
                "Packet.id",
                raw_child.clone(),
                json!({}),
                json!("child"),
            ),
            (
                vec!["child", "label"],
                "Packet.label",
                raw_child.clone(),
                json!({"prefix":"dto:"}),
                json!("dto:child"),
            ),
        ] {
            let mut path = vec![alias];
            path.extend(suffix);
            let pair: Vec<_> = logs
                .iter()
                .enumerate()
                .filter(|(_, (_, fields))| fields["field"]["path"] == json!(path))
                .collect();
            assert_eq!(pair.len(), 2, "{path:?}");
            assert_eq!(pair[0].1.0, FieldBefore);
            assert_eq!(pair[1].1.0, FieldAfter);
            let field =
                json!({"coordinate":coordinate,"path":path,"arguments":args,"parent":parent});
            assert_eq!(pair[0].1.1["field"], field);
            assert_eq!(pair[1].1.1["field"], field);
            assert_eq!(pair[1].1.1["result"], json!({"value":value,"errors":[]}));
            if path.len() > 1 {
                let root_after = logs
                    .iter()
                    .position(|(slot, fields)| {
                        *slot == FieldAfter && fields["field"]["path"] == json!([alias])
                    })
                    .unwrap();
                assert!(
                    root_after < pair[0].0,
                    "child field must run only after the root outcome is observed"
                );
            }
        }
    }
    fixture.events.lock().unwrap().clear();
    assert_eq!(
        fixture.query("{ __typename }").await,
        (json!({"data":{"__typename":"Query"}}), None)
    );
    assert_eq!(
        fixture
            .logs()
            .iter()
            .map(|(slot, _)| *slot)
            .collect::<Vec<_>>(),
        [RequestBefore, OperationBefore]
    );
    fixture.stop().await;
}
