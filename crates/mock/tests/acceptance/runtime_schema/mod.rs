//! Verify a second domain and renames through initialization, editing, offline checks, and real HTTP.

mod config_contract;
mod domain;
mod init_contract;
mod model_api;
mod model_api_fixture;
mod numeric_literals;
mod rename;
mod representation;
mod runtime_boundary;
mod scaffold;

use crate::support::{Directory, Server, seed};
use serde_json::{Value, json};
use std::fs;

const PROJECTS: &str = include_str!("../../../fixtures/projects.graphql");

pub(super) fn init(source: &str) -> (Directory, String) {
    let directory = Directory::new();
    fs::write(directory.0.join("external.graphql"), source).unwrap();
    let result = directory.command(&[
        "init",
        "--schema",
        "external.graphql",
        "--config-dir",
        "mock",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let config = scaffold::bundle(&directory.0.join("mock"), source);
    fs::write(directory.0.join("mock.toml"), &config).unwrap();
    // Build self-contained bundle fixtures in the test and pass only TOML to the binary.
    fs::remove_file(directory.0.join("external.graphql")).unwrap();
    (directory, config)
}

pub(super) fn ready(config: &str, edits: &[(&str, &str)]) -> String {
    let mut config = config.parse::<toml_edit::DocumentMut>().unwrap();
    for (coordinate, body) in edits {
        assert!(
            config["resolvers"].get(coordinate).is_some(),
            "{coordinate}"
        );
        config["resolvers"][coordinate]["luau"] =
            toml_edit::value(crate::support::module(&config, coordinate, body));
    }
    config.to_string()
}

pub(super) fn projects() -> String {
    projects_source(PROJECTS)
}

fn projects_source(source: &str) -> String {
    let (directory, config) = init(source);
    assert_eq!(
        seed(&config),
        json!({"Member":[],"Packet":[],"Project":[],"Task":[]})
    );
    let result = directory.command(&["check", "--config", "mock.toml"]);
    assert!(result.status.success());
    ready(
        &config,
        &[
            ("Member.label", "return args.prefix .. parent.name"),
            ("Packet.id", "return parent.caption"),
            ("Packet.label", "return 'packet:' .. parent.caption"),
            (
                "Query.members",
                "if args.status == nil or args.status == ctx.null then return ctx.models.Member:find({limit=args.limit :: number}) end return ctx.models.Member:find({where={field='status',op='eq',value=args.status :: ChigiriStatusValue},limit=args.limit :: number})",
            ),
            ("Query.projects", "return ctx.models.Project:find({})"),
            ("Query.tasks", "return ctx.models.Task:find({})"),
            (
                "Query.result",
                "local id = args.pick.id\nif id ~= nil then return ctx.models.Member:find({where = {field = 'id', op = 'eq', value = id}})[1] or ctx.null end\nreturn {__typename = 'Packet' :: 'Packet', caption = args.pick.name or 'missing'}",
            ),
            (
                "Query.entity",
                "return ctx.models.Member:find({limit=1})[1] or ctx.null",
            ),
            (
                "Query.packet",
                "local m = ctx.models.Member:find({limit=1})[1] return {caption='outer', child={caption='inner'}, member={id=m.id,name=m.name,status=m.status,code=m.code}, data=ctx.jsonObject({empty=ctx.jsonArray({}), missing=ctx.null})}",
            ),
            (
                "Query.echo",
                "if args.data == nil then return ctx.null end return args.data",
            ),
            ("Query.count", "return args.value"),
            (
                "Mutation.addMember",
                "if ctx.scope == 'mutation' then local id = 'Member-' .. (ctx.models.Member:count({}) + 1) return ctx.models.Member:create({id=id,name=args.input.name,status=args.input.status,code=args.input.code,managerId=args.input.managerId}) else error('write requires mutation root') end",
            ),
            (
                "Mutation.changeMember",
                "if ctx.scope == 'mutation' then return ctx.models.Member:update({where={field='id',op='eq',value=args.id}}, args.input)[1] else error('write requires mutation root') end",
            ),
            (
                "Mutation.removeMember",
                "if ctx.scope == 'mutation' then return ctx.models.Member:delete({where={field='id',op='eq',value=args.id}}) > 0 else error('write requires mutation root') end",
            ),
            (
                "Mutation.addProject",
                "if ctx.scope == 'mutation' then local id = 'Project-' .. (ctx.models.Project:count({}) + 1) return ctx.models.Project:create({id=id,name=args.input.name,leadId=args.input.leadId,memberIds=args.input.memberIds}) else error('write requires mutation root') end",
            ),
            (
                "Mutation.changeProject",
                "if ctx.scope == 'mutation' then return ctx.models.Project:update({where={field='id',op='eq',value=args.id}}, args.input)[1] else error('write requires mutation root') end",
            ),
            (
                "Mutation.removeProject",
                "if ctx.scope == 'mutation' then return ctx.models.Project:delete({where={field='id',op='eq',value=args.id}}) > 0 else error('write requires mutation root') end",
            ),
            (
                "Mutation.addTask",
                "if ctx.scope == 'mutation' then local id = 'Task-' .. (ctx.models.Task:count({}) + 1) return ctx.models.Task:create({id=id,title=args.input.title,projectId=args.input.projectId,assigneeId=args.input.assigneeId}) else error('write requires mutation root') end",
            ),
            (
                "Mutation.changeTask",
                "if ctx.scope == 'mutation' then return ctx.models.Task:update({where={field='id',op='eq',value=args.id}}, args.input)[1] else error('write requires mutation root') end",
            ),
            (
                "Mutation.removeTask",
                "if ctx.scope == 'mutation' then return ctx.models.Task:delete({where={field='id',op='eq',value=args.id}}) > 0 else error('write requires mutation root') end",
            ),
        ],
    )
}

fn data(server: &Server, query: &str) -> Value {
    let result = server.graphql(query);
    assert!(result.get("errors").is_none(), "{query}: {result}");
    result["data"].clone()
}

fn failure(server: &Server, query: &str) -> Value {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":query}).to_string(),
    );
    assert!(matches!(response.status, 500 | 422), "{}", response.body);
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert!(
        result["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "{query}: {result}"
    );
    if response.status == 500 {
        assert!(result["data"].is_null());
        assert_eq!(result["errors"].as_array().unwrap().len(), 1);
        let error = &result["errors"][0];
        let message = match error["extensions"]["code"].as_str().unwrap() {
            "MOCK_EXECUTION_FAILED" => "Mock execution failed",
            "MOCK_EXECUTION_TIMEOUT" => "Mock execution timed out",
            "MOCK_EXECUTION_LIMIT_EXCEEDED" => "Mock execution limit exceeded",
            code => panic!("unexpected public error code: {code}"),
        };
        assert_eq!(error["message"], message);
        assert!(
            error["extensions"]["requestId"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
    }
    result
}

fn populate(server: &Server) {
    assert_eq!(
        data(
            server,
            r#"mutation {
        a:addMember(input:{name:"Ada",code:"AA"}) {id status manager{id}}
        b:addMember(input:{name:"Ben",code:"BB",managerId:"Member-1"}) {id status}
    }"#
        ),
        json!({"a":{"id":"Member-1","status":"OPEN","manager":null},"b":{"id":"Member-2","status":"OPEN"}})
    );
    assert_eq!(
        data(
            server,
            r#"mutation {
        a:addProject(input:{name:"One",leadId:"Member-1",memberIds:["Member-2","Member-1"]}) {id}
        b:addProject(input:{name:"Two",memberIds:["Member-1"]}) {id lead{id}}
        t:addTask(input:{title:"First",projectId:"Project-1",assigneeId:"Member-2"}) {id}
    }"#
        ),
        json!({"a":{"id":"Project-1"},"b":{"id":"Project-2","lead":null},"t":{"id":"Task-1"}})
    );
}
