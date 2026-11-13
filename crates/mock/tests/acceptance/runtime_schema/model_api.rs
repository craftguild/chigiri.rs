//! Verify seven model operations, GraphQL selection, and full-model rollback over real HTTP.

mod simplified;

use super::{data, failure, model_api_fixture, populate};
use crate::support::Server;
use serde_json::{Value, json};
use std::{
    io::Write,
    net::TcpStream,
    time::{Duration, Instant},
};

fn snapshot(server: &Server) -> Value {
    // Cover every field while splitting the evidence across 16 queued selections.
    json!({
        "counts": data(server, "{memberCount projectCount}"),
        "members": data(server, "{members{id name status code}}"),
        "member_edges": data(server, "{members{manager{id} reports{id} leading{id} projects{id}}}"),
        "projects": data(server, "{projects{id name lead{id} tasks{id}}}"),
        "project_edges": data(server, "{projects{members{id}}}"),
        "tasks": data(server, "{tasks{id title project{id} assignee{id}}}")
    })
}

#[test]
fn mr_03_06_10_17_24_25_seven_operations_keep_variables_alias_fragments_abstract_and_null() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(&model_api_fixture::config());
    populate(&server);
    let response = server.operation(json!({
        "query": "query($needle:String!,$skip:Int!){matches:searchMembers(needle:$needle,offset:$skip,limit:1){...MemberInfo} total:memberCount(status:OPEN) done:memberCount(status:DONE)} fragment MemberInfo on Member{id code name}",
        "variables": {"needle":"A", "skip":0}
    }));
    assert!(response.get("errors").is_none(), "{response}");
    assert_eq!(
        response["data"],
        json!({"matches":[{"id":"Member-1","code":"AA","name":"Ada"}],"total":2,"done":0})
    );
    assert_eq!(
        data(
            &server,
            "{searchMembers(needle:\"\",offset:1000){id} memberCount}"
        ),
        json!({"searchMembers":[],"memberCount":2})
    );
    assert_eq!(
        data(
            &server,
            r#"mutation {changed:changeMember(id:"Member-1",input:{name:"Ada-new"}){id name}}"#
        ),
        json!({"changed":{"id":"Member-1","name":"Ada-new"}})
    );
    let linked = data(
        &server,
        r#"mutation {p:linkProject(id:"Project-2",memberId:"Member-2"){id members{id projects{id}}}}"#,
    );
    assert_eq!(
        linked,
        json!({"p":{"id":"Project-2","members":[
            {"id":"Member-1","projects":[{"id":"Project-1"},{"id":"Project-2"}]},
            {"id":"Member-2","projects":[{"id":"Project-1"},{"id":"Project-2"}]}
        ]}})
    );
    assert_eq!(
        data(
            &server,
            r#"mutation {p:linkProject(id:"Project-2",memberId:"Member-2"){members{id}}}"#
        ),
        json!({"p":{"members":[{"id":"Member-1"},{"id":"Member-2"}]}})
    );
    for target in ["Member-1", "absent-target"] {
        assert_eq!(
            data(
                &server,
                &format!(
                    "mutation {{p:unlinkProject(id:\"Project-2\",memberId:\"{target}\"){{members{{id}}}}}}"
                )
            ),
            json!({"p":{"members":[{"id":"Member-2"}]}})
        );
    }
    assert_eq!(
        data(
            &server,
            r#"mutation {p:unlinkProject(id:"Project-2",memberId:"Member-2"){members{id}}}"#
        ),
        json!({"p":{"members":[]}})
    );
    assert_eq!(
        data(
            &server,
            r#"mutation {new:addMember(input:{name:"Temporary",code:"CC"}){id} gone:removeMember(id:"Member-3")}"#
        ),
        json!({"new":{"id":"Member-3"},"gone":true})
    );
    let abstract_values = data(
        &server,
        r#"{
        person:result(pick:{id:"Member-1"}){__typename ...MemberInfo}
        dto:result(pick:{name:"packet"}){__typename ... on Packet{id child{id} label}}
        missing:result(pick:{id:"absent"}){__typename}
        entity{__typename id ... on Member{name}}
    } fragment MemberInfo on Member{id name label}"#,
    );
    assert_eq!(
        abstract_values,
        json!({
            "person":{"__typename":"Member","id":"Member-1","name":"Ada-new","label":"member:Ada-new"},
            "dto":{"__typename":"Packet","id":"packet","child":null,"label":"packet:packet"},
            "missing":null,"entity":{"__typename":"Member","id":"Member-1","name":"Ada-new"}
        })
    );
    let repeated = data(
        &server,
        "{searchMembers(needle:\"\",offset:1,limit:1){id code}}",
    );
    assert_eq!(
        repeated,
        json!({"searchMembers":[{"id":"Member-2","code":"BB"}]})
    );
    assert_eq!(
        repeated,
        data(
            &server,
            "{searchMembers(needle:\"\",offset:1,limit:1){id code}}"
        )
    );
    server.stop("-TERM");
}

#[test]
fn mr_09_13_14_15_http_relation_changes_roll_back_on_host_return_script_work_and_vm_failure() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(&model_api_fixture::config());
    populate(&server);
    let before = snapshot(&server);
    for mode in ["host", "return", "script", "work", "loop"] {
        let response = failure(
            &server,
            &format!(
                "mutation {{failed:failProject(id:\"Project-2\",memberId:\"Member-2\",mode:\"{mode}\")}}"
            ),
        );
        assert!(response["data"].is_null());
        assert_eq!(snapshot(&server), before, "{mode}");
    }
    for operation in [
        "{forbidden}",
        "{root{unchecked(id:\"Project-2\",memberId:\"Member-2\")}}",
    ] {
        failure(&server, operation);
        assert_eq!(snapshot(&server), before);
    }
    assert_eq!(
        data(
            &server,
            r#"mutation {
        m:addMember(input:{name:"Recovered",code:"RC"}){id}
        p:addProject(input:{name:"Recovered",memberIds:[]}){id}
    }"#
        ),
        json!({"m":{"id":"Member-3"},"p":{"id":"Project-3"}})
    );
    assert_eq!(
        data(&server, "{memberCount projectCount}"),
        json!({"memberCount":3,"projectCount":3})
    );
    server.stop("-TERM");
}

#[test]
fn mr_13_15_http_cancelled_relation_mutation_recovers_the_same_server_and_ids() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(&model_api_fixture::config());
    populate(&server);
    let before = snapshot(&server);
    let body = json!({"query":"mutation {failProject(id:\"Project-2\",memberId:\"Member-2\",mode:\"loop\")}"}).to_string();
    let mut stream = TcpStream::connect(server.address).unwrap();
    write!(stream, "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    drop(stream);
    let resumed = Instant::now();
    assert_eq!(snapshot(&server), before);
    assert!(resumed.elapsed() < Duration::from_secs(2));
    assert_eq!(
        data(
            &server,
            r#"mutation {m:addMember(input:{name:"After cancel",code:"AC"}){id} p:addProject(input:{name:"After cancel",memberIds:[]}){id}}"#
        ),
        json!({"m":{"id":"Member-3"},"p":{"id":"Project-3"}})
    );
    server.stop("-TERM");
}
