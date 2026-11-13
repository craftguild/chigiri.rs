//! Verify runtime-schema GraphQL inputs over real HTTP.

use super::{data, failure, populate, projects};
use crate::support::Server;
use serde_json::json;

#[test]
fn rsm_05_09_11_second_domain_crud_all_relation_shapes_and_restrict_over_http() {
    let _test_case = crate::support::test_case();
    let mut server = Server::offline(&projects());
    populate(&server);
    assert_eq!(
        data(
            &server,
            "{ projects { id lead{id} members{id} tasks{id assignee{id}} } }"
        ),
        json!({"projects":[
            {"id":"Project-1","lead":{"id":"Member-1"},"members":[{"id":"Member-2"},{"id":"Member-1"}],"tasks":[{"id":"Task-1","assignee":{"id":"Member-2"}}]},
            {"id":"Project-2","lead":null,"members":[{"id":"Member-1"}],"tasks":[]}
        ]})
    );
    assert_eq!(
        data(
            &server,
            "{ members { id leading{id} projects{id} manager{id} reports{id} } }"
        ),
        json!({"members":[
            {"id":"Member-1","leading":{"id":"Project-1"},"projects":[{"id":"Project-1"},{"id":"Project-2"}],"manager":null,"reports":[{"id":"Member-2"}]},
            {"id":"Member-2","leading":null,"projects":[{"id":"Project-1"}],"manager":{"id":"Member-1"},"reports":[]}
        ]})
    );
    for query in [
        r#"mutation { addProject(input:{name:"duplicate lead",leadId:"Member-1",memberIds:[]}){id} }"#,
        r#"mutation { removeMember(id:"Member-1") }"#,
        r#"mutation { removeProject(id:"Project-1") }"#,
        r#"mutation { changeProject(id:"Project-1",input:{memberIds:["absent"]}){id} }"#,
    ] {
        failure(&server, query);
    }
    assert_eq!(
        data(
            &server,
            r#"mutation {
        p:changeProject(id:"Project-1",input:{leadId:null,memberIds:["Member-2"]}) {id lead{id} members{id}}
        q:changeProject(id:"Project-2",input:{leadId:"Member-1"}) {id lead{id}}
        m:changeMember(id:"Member-2",input:{managerId:null,status:DONE}) {id status manager{id}}
        t:changeTask(id:"Task-1",input:{projectId:"Project-2",assigneeId:null}) {id project{id} assignee{id}}
    }"#
        ),
        json!({"p":{"id":"Project-1","lead":null,"members":[{"id":"Member-2"}]},"q":{"id":"Project-2","lead":{"id":"Member-1"}},"m":{"id":"Member-2","status":"DONE","manager":null},"t":{"id":"Task-1","project":{"id":"Project-2"},"assignee":null}})
    );
    assert_eq!(
        data(
            &server,
            "{ members(status:DONE){id} projects{id tasks{id}} }"
        ),
        json!({"members":[{"id":"Member-2"}],"projects":[{"id":"Project-1","tasks":[]},{"id":"Project-2","tasks":[{"id":"Task-1"}]}]})
    );
    assert_eq!(
        data(
            &server,
            r#"mutation {
        t:removeTask(id:"Task-1") p:removeProject(id:"Project-1") q:removeProject(id:"Project-2")
        m:removeMember(id:"Member-1") n:removeMember(id:"Member-2")
        next:addProject(input:{name:"next",memberIds:[]}){id}
    }"#
        ),
        json!({"t":true,"p":true,"q":true,"m":true,"n":true,"next":{"id":"Project-1"}})
    );
    assert_eq!(
        data(&server, "{ members{id} tasks{id} projects{id} }"),
        json!({"members":[],"tasks":[],"projects":[{"id":"Project-1"}]})
    );
    server.stop("-TERM");
}

#[test]
fn rsm_04_07_14_18_19_offline_abstract_payload_oneof_and_custom_scalar_http() {
    let _test_case = crate::support::test_case();
    let mut server = Server::offline(&projects());
    populate(&server);
    assert_eq!(
        data(
            &server,
            r#"query {
        selected:result(pick:{id:"Member-2"}) { __typename ...M }
        dto:result(pick:{name:"chosen"}) { __typename ... on Packet{id label} }
        entity { __typename id ...M }
        packet {id label child{id label} member{label(prefix:"hello:")} data}
        count
    } fragment M on Member { id label status }"#
        ),
        json!({
            "selected":{"__typename":"Member","id":"Member-2","label":"member:Ben","status":"OPEN"},
            "entity":{"__typename":"Member","id":"Member-1","label":"member:Ada","status":"OPEN"},
            "dto":{"__typename":"Packet","id":"chosen","label":"packet:chosen"},
            "packet":{"id":"outer","label":"packet:outer","child":{"id":"inner","label":"packet:inner"},"member":{"label":"hello:Ada"},"data":{"empty":[],"missing":null}},
            "count":9007199254740994_i64
        })
    );
    let container = json!({"array":[null,{},[],true,9007199254740994_i64],"null":null});
    let response = server.operation(
        json!({"query":"query($data:Data){echo(data:$data)}", "variables":{"data":container}}),
    );
    assert_eq!(response, json!({"data":{"echo":container}}));
    assert_eq!(
        data(
            &server,
            "{ missing:echo explicit:echo(data:null) empty:echo(data:{}) list:echo(data:[]) }"
        ),
        json!({"missing":null,"explicit":null,"empty":{},"list":[]})
    );
    for query in [
        "{result(pick:{}){__typename}}",
        "{result(pick:{id:null}){__typename}}",
        "{result(pick:{id:\"Member-1\",name:\"x\"}){__typename}}",
        "{count(value:-1)}",
        "{count(value:9007199254740993)}",
        r#"mutation {addMember(input:{name:"bad",code:"x"}){id}}"#,
        r#"mutation {addMember(input:{name:"bad",code:"TOO-LONG-CODE"}){id}}"#,
    ] {
        failure(&server, query);
    }
    assert_eq!(
        data(
            &server,
            r#"mutation {addMember(input:{name:"next",code:"OK"}){id}}"#
        ),
        json!({"addMember":{"id":"Member-3"}})
    );
    server.stop("-INT");
}
