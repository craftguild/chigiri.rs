//! Model output identity remains host-owned across user-authored containers.

use chigiri::core::Object;

use super::tests::{contract, record, run, script};
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;

#[test]
fn sa_05_connection_nodes_and_edges_preserve_records_and_relations() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type User { id: ID! posts: [Post!]! @relation }
        type Post { id: ID! author: User! @relation(fields:["authorId"], references:["id"]) }
        type Edge { node: Post! }
        type Connection { nodes: [Post!]! edges: [Edge!]! }
        type UserEdge { node: User! }
        type UserConnection { nodes: [User!]! edges: [UserEdge!]! }
        type Query { connection: Connection! users: UserConnection! }
        type Mutation { seed: Boolean! }
        "#,
    );
    let seed = script(
        &contract,
        "Mutation.seed",
        "local user=ctx.models.User:create({id='u'}); ctx.models.Post:create({id='p',authorId=user.id}); return true",
    );
    let connection = script(
        &contract,
        "Query.connection",
        "local rows=ctx.models.Post:find({}); return {nodes=rows,edges={{node=rows[1]}}}",
    );
    let mut store = Store::empty(contract.ir().clone());
    run(&seed, &Object::new(), &mut store).unwrap();
    let connection = record(run(&connection, &Object::new(), &mut store).unwrap());
    let ObjectData::Dto(fields) = connection.data() else {
        panic!("connection must remain a DTO");
    };
    let graph = contract.ir().graph();
    let connection_type = graph.type_id("Connection").unwrap();
    let nodes = graph.types()[connection_type.0].fields["nodes"];
    let edges = graph.types()[connection_type.0].fields["edges"];
    let OutputValue::List(nodes) = &fields[&nodes] else {
        panic!("nodes list required");
    };
    let OutputValue::Object(node) = &nodes[0] else {
        panic!("node record required");
    };
    let ObjectData::Model(post) = node.data() else {
        panic!("nodes must preserve model identity");
    };
    let OutputValue::List(edges) = &fields[&edges] else {
        panic!("edges list required");
    };
    let OutputValue::Object(edge) = &edges[0] else {
        panic!("edge object required");
    };
    let ObjectData::Dto(edge_fields) = edge.data() else {
        panic!("edge must remain a DTO");
    };
    let edge_type = graph.type_id("Edge").unwrap();
    let edge_node = graph.types()[edge_type.0].fields["node"];
    assert!(matches!(
        &edge_fields[&edge_node],
        OutputValue::Object(value) if matches!(value.data(), ObjectData::Model(_))
    ));

    let post_type = graph.type_id("Post").unwrap();
    let author = graph.types()[post_type.0].fields["author"];
    let related = node
        .snapshot()
        .reader()
        .related_output(author, post.row())
        .unwrap();
    assert!(matches!(
        related,
        OutputValue::Object(value) if matches!(value.data(), ObjectData::Model(_))
    ));

    let users = script(
        &contract,
        "Query.users",
        "local rows=ctx.models.User:find({}); return {nodes=rows,edges={{node=rows[1]}}}",
    );
    let users = record(run(&users, &Object::new(), &mut store).unwrap());
    let ObjectData::Dto(fields) = users.data() else {
        panic!("user connection must remain a DTO");
    };
    let user_connection = graph.type_id("UserConnection").unwrap();
    let user_nodes = graph.types()[user_connection.0].fields["nodes"];
    let user_edges = graph.types()[user_connection.0].fields["edges"];
    let OutputValue::List(nodes) = &fields[&user_nodes] else {
        panic!("user nodes list required");
    };
    let OutputValue::Object(user) = &nodes[0] else {
        panic!("user record required");
    };
    assert!(matches!(user.data(), ObjectData::Model(_)));
    let OutputValue::List(edges) = &fields[&user_edges] else {
        panic!("user edges list required");
    };
    let OutputValue::Object(edge) = &edges[0] else {
        panic!("user edge object required");
    };
    let ObjectData::Dto(edge_fields) = edge.data() else {
        panic!("user edge must remain a DTO");
    };
    let user_edge = graph.type_id("UserEdge").unwrap();
    let user_edge_node = graph.types()[user_edge.0].fields["node"];
    assert!(matches!(
        &edge_fields[&user_edge_node],
        OutputValue::Object(value) if matches!(value.data(), ObjectData::Model(_))
    ));

    let user_type = graph.type_id("User").unwrap();
    let posts = graph.types()[user_type.0].fields["posts"];
    let ObjectData::Model(user_record) = user.data() else {
        unreachable!()
    };
    let inverse = user
        .snapshot()
        .reader()
        .related_output(posts, user_record.row())
        .unwrap();
    assert!(matches!(
        inverse,
        OutputValue::List(values)
            if matches!(&values[0], OutputValue::Object(value)
                if matches!(value.data(), ObjectData::Model(_)))
    ));
}

#[test]
fn sa_05_forged_modified_and_stale_record_tables_are_rejected() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "type Item{id:ID! name:String!} type Query{item:Item!} type Mutation{seed:Item! replace:Item!}",
    );
    let mut store = Store::empty(contract.ir().clone());
    let seed = script(
        &contract,
        "Mutation.seed",
        "return ctx.models.Item:create({id='i',name='original'})",
    );
    run(&seed, &Object::new(), &mut store).unwrap();

    for body in [
        "local row=ctx.models.Item:find({})[1]; return {id=row.id,name=row.name}",
        "local row=ctx.models.Item:find({})[1]; row.name='changed'; return row",
    ] {
        let forged = script(&contract, "Query.item", body);
        assert!(run(&forged, &Object::new(), &mut store).is_err(), "{body}");
    }

    let stale = script(
        &contract,
        "Mutation.replace",
        "local old=ctx.models.Item:find({})[1]; ctx.models.Item:update({where={field='id',op='eq',value='i'}},{name='new'}); return old",
    );
    assert!(run(&stale, &Object::new(), &mut store).is_err());
    let item = contract
        .ir()
        .model_for(contract.ir().graph().type_id("Item").unwrap())
        .unwrap();
    let name = contract.ir().models()[item.0].slots["name"];
    assert_eq!(
        store
            .snapshot()
            .record(item, "i")
            .unwrap()
            .get(name)
            .unwrap()
            .as_str(),
        Some("original")
    );
}

#[test]
fn ab_08_nested_payload_edges_and_reordered_lists_preserve_record_identity() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type Item { id:ID! }
        type Edge { node:Item! }
        type Connection { edges:[Edge!]! }
        type Payload { connection:Connection! reordered:[Item!]! }
        type Query { payload:Payload! }
        type Mutation { seed:Boolean! }
        "#,
    );
    let seed = script(
        &contract,
        "Mutation.seed",
        "ctx.models.Item:create({id='a'}); ctx.models.Item:create({id='b'}); return true",
    );
    let payload = script(
        &contract,
        "Query.payload",
        "local rows=ctx.models.Item:find({}); return {connection={edges={{node=rows[2]},{node=rows[1]}}},reordered={rows[2],rows[1]}}",
    );
    let mut store = Store::empty(contract.ir().clone());
    run(&seed, &Object::new(), &mut store).unwrap();
    let payload = record(run(&payload, &Object::new(), &mut store).unwrap());
    let ObjectData::Dto(payload_fields) = payload.data() else {
        panic!("payload DTO required")
    };
    let graph = contract.ir().graph();
    let payload_type = graph.type_id("Payload").unwrap();
    let connection_field = graph.types()[payload_type.0].fields["connection"];
    let reordered_field = graph.types()[payload_type.0].fields["reordered"];
    let OutputValue::Object(connection) = &payload_fields[&connection_field] else {
        panic!("connection DTO required")
    };
    let ObjectData::Dto(connection_fields) = connection.data() else {
        panic!("connection fields required")
    };
    let connection_type = graph.type_id("Connection").unwrap();
    let edges_field = graph.types()[connection_type.0].fields["edges"];
    let OutputValue::List(edges) = &connection_fields[&edges_field] else {
        panic!("edges required")
    };
    let edge_type = graph.type_id("Edge").unwrap();
    let node_field = graph.types()[edge_type.0].fields["node"];
    let edge_ids = edges
        .iter()
        .map(|edge| {
            let OutputValue::Object(edge) = edge else {
                panic!("edge object required")
            };
            let ObjectData::Dto(fields) = edge.data() else {
                panic!("edge DTO required")
            };
            let OutputValue::Object(node) = &fields[&node_field] else {
                panic!("edge node required")
            };
            let ObjectData::Model(record) = node.data() else {
                panic!("record identity required")
            };
            record.id().unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(edge_ids, ["b", "a"]);
    let OutputValue::List(reordered) = &payload_fields[&reordered_field] else {
        panic!("reordered list required")
    };
    let reordered_ids = reordered
        .iter()
        .map(|value| {
            let OutputValue::Object(value) = value else {
                panic!("record required")
            };
            let ObjectData::Model(record) = value.data() else {
                panic!("record identity required")
            };
            record.id().unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(reordered_ids, ["b", "a"]);
}
