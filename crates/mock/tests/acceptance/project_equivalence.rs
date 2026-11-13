//! Compare both domains across initial state, typed execution, full rollback, and restart.

use std::fs;

use serde_json::{Value, json};

use crate::support::{Directory, Server, TEMPLATE, module, with_seed};

struct Domain {
    source: &'static str,
    config: String,
    query: &'static str,
    fail: &'static str,
    success: &'static str,
    root: &'static str,
    next_id: &'static str,
    subsequent: Vec<(&'static str, &'static str, &'static str)>,
}

fn edited(config: &str, coordinate: &str, body: &str) -> String {
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    let source = module(&document, coordinate, body);
    document["resolvers"][coordinate]["luau"] = toml_edit::value(source);
    document.to_string()
}

fn domains() -> Vec<Domain> {
    let blog = edited(
        TEMPLATE,
        "Mutation.createUser",
        r#"
local user = ctx.models.User:create({id='User-' .. (ctx.models.User:count({}) + 1), name=args.input.name})
if args.input.name == "fail" then
    ctx.models.Post:create({id='Post-' .. (ctx.models.Post:count({}) + 1),title="temporary",body="temporary",authorId=user.id})
    ctx.models.Post:update({where={field='id',op='eq',value="Post-1"}}, {title="temporary",authorId=user.id})
    error("private rollback value")
end
return user
"#,
    );
    let projects = with_seed(
        &crate::runtime_schema::projects(),
        &json!({
            "Member":[{"id":"Member-9","name":"Ada","status":"OPEN","code":"AA"}],
            "Project":[{"id":"Project-7","name":"Initial","leadId":"Member-9","memberIds":["Member-9"]}],
            "Task":[{"id":"Task-5","title":"Initial","projectId":"Project-7","assigneeId":"Member-9"}]
        }),
    );
    let projects = edited(
        &projects,
        "Mutation.addMember",
        r#"
local member = ctx.models.Member:create({id='Member-' .. (ctx.models.Member:count({}) + 1), name=args.input.name, status=args.input.status, code=args.input.code, managerId=args.input.managerId})
if args.input.name == "fail" then
    local project = ctx.models.Project:create({id='Project-' .. (ctx.models.Project:count({}) + 1),name="temporary",leadId=member.id,memberIds={member.id}})
    ctx.models.Task:create({id='Task-' .. (ctx.models.Task:count({}) + 1),title="temporary",projectId=project.id,assigneeId=member.id})
    ctx.models.Task:update({where={field='id',op='eq',value="Task-5"}}, {title="temporary",assigneeId=member.id})
    error("private rollback value")
end
return member
"#,
    );
    vec![
        Domain {
            source: include_str!("../../fixtures/blog.graphql"),
            config: blog,
            root: "createUser",
            next_id: "User-3",
            query: "{getUsers{id name posts{id title author{id}}} findPost{id title body author{id}}}",
            fail: "mutation{createUser(input:{name:\"fail\"}){id}}",
            success: "mutation{createUser(input:{name:\"success\"}){id name}}",
            subsequent: vec![(
                "mutation{createPost(input:{title:\"after\",body:\"\",authorId:\"User-2\"}){id}}",
                "createPost",
                "Post-2",
            )],
        },
        Domain {
            source: include_str!("../../fixtures/projects.graphql"),
            config: projects,
            root: "addMember",
            next_id: "Member-2",
            query: "{members{id name projects{id name tasks{id title assignee{id}}}} projects{id name lead{id} members{id}} tasks{id title project{id} assignee{id}}}",
            fail: "mutation{addMember(input:{name:\"fail\",code:\"BB\"}){id}}",
            success: "mutation{addMember(input:{name:\"success\",code:\"BB\"}){id name}}",
            subsequent: vec![
                (
                    "mutation{addProject(input:{name:\"after\",memberIds:[\"Member-9\"]}){id}}",
                    "addProject",
                    "Project-2",
                ),
                (
                    "mutation{addTask(input:{title:\"after\",projectId:\"Project-7\"}){id}}",
                    "addTask",
                    "Task-2",
                ),
            ],
        },
    ]
}

fn sequence(server: &Server, domain: &Domain) -> Vec<Value> {
    let initial = server.graphql(domain.query);
    assert!(initial.get("errors").is_none(), "{initial}");
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":domain.fail}).to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let mut failure: Value = serde_json::from_str(&response.body).unwrap();
    assert!(failure["data"].is_null(), "{failure}");
    let errors = failure["errors"].as_array_mut().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["message"], "Mock execution failed");
    assert_eq!(errors[0]["extensions"]["code"], "MOCK_EXECUTION_FAILED");
    let id = errors[0]["extensions"]["requestId"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(id > 0);
    // Both inputs return the same sanitized public response except for the request ID.
    errors[0]["extensions"]
        .as_object_mut()
        .unwrap()
        .remove("requestId");
    assert_eq!(server.graphql(domain.query), initial);
    let success = server.graphql(domain.success);
    assert!(success.get("errors").is_none(), "{success}");
    assert_eq!(success["data"][domain.root]["id"], domain.next_id);
    let mut outputs = vec![initial, failure, success];
    for (query, root, id) in &domain.subsequent {
        let result = server.graphql(query);
        assert!(result.get("errors").is_none(), "{result}");
        assert_eq!(result["data"][root]["id"], *id);
        outputs.push(result);
    }
    outputs.push(server.graphql(domain.query));
    outputs
}

#[test]
fn mp_23_29_30_two_domains_match_errors_all_model_and_id_rollback_and_restart_initial_state() {
    let _test_case = crate::support::test_case();
    for domain in domains() {
        let directory = crate::project_workflow::project(domain.source, &domain.config);
        let output = directory.command(&[
            "export",
            "--config-dir",
            "mock",
            "--output",
            "portable.toml",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bundle = fs::read_to_string(directory.0.join("portable.toml")).unwrap();
        let mut server = Server::project(directory);
        let expected = sequence(&server, &domain);
        server.stop("-TERM");
        let mut restarted = Server::project(Directory(server.directory.0.clone()));
        assert_eq!(sequence(&restarted, &domain), expected);
        restarted.stop("-TERM");
        fs::rename(
            server.directory.0.join("mock"),
            server.directory.0.join("unavailable-project"),
        )
        .unwrap();
        fs::remove_file(server.directory.0.join("schema.graphql")).unwrap();
        let mut offline = Server::offline(&bundle);
        assert_eq!(sequence(&offline, &domain), expected);
        offline.stop("-TERM");
        let mut restarted = Server::offline(&bundle);
        assert_eq!(sequence(&restarted, &domain), expected);
        restarted.stop("-TERM");
    }
}
