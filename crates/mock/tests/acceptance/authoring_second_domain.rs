//! Prove authoring boundaries with renamed roots and domain types through the real CLI.

use std::fs;

use chigiri_mock::luau::render_module;
use chigiri_mock::project::{Layout, read_project_schema};
use serde_json::{Value, json};

use crate::support::{Directory, Server};

const PUBLIC: &str = r#"
schema { query: Directory mutation: Commands }
type Media { id:ID! src:String! viewerLabel:String! @virtual }
type Treasury { id:ID! credits:Int! }
type Member {
  id:ID! display:String! media:[Media!]! treasuryCode:ID!
  treasury:Treasury! @relation(fields:["treasuryCode"],references:["id"])
}
type MemberEdge { node:Member! }
type MemberConnection { edges:[MemberEdge!]! }
type Directory { roster:MemberConnection! }
type Commands { forgeMedia:Media! }
"#;

const STORAGE: &str = r#"
extend type Media { ownerCode:ID! }
extend type Member { rank:Int! }
"#;

#[test]
fn ab_14_renamed_domain_keeps_typing_embedding_provenance_and_discovery() {
    let _case = crate::support::test_case();
    let directory = build();
    assert_success(
        &directory,
        &[
            "check",
            "--config-dir",
            "mock",
            "--target",
            "resolvers/query/roster.luau",
        ],
    );
    assert_success(&directory, &["check", "--config-dir", "mock"]);

    for (topic, name, kind) in [
        ("models", "Member", "models"),
        ("types", "Media", "types"),
        ("resolvers", "Directory.roster", "resolvers"),
    ] {
        let output = directory.command(&[
            "reference",
            topic,
            name,
            "--config-dir",
            "mock",
            "--format",
            "json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document["format"], "chigiri-mock-reference-topic/v9");
        assert_eq!(document["selection"], json!({"topic":topic,"name":name}));
        assert_eq!(document["content"]["kind"], kind);
        let identity = if topic == "resolvers" {
            "coordinate"
        } else {
            "name"
        };
        assert_eq!(document["content"]["items"][0][identity], name);
        assert!(!document.to_string().contains("../graphql"));
    }

    let mut server = Server::project_with_graphiql(directory);
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json"), ("x-viewer", "guest")],
        &json!({"query":"{roster{edges{node{id display media{id src viewerLabel} treasury{id credits}}}}}"}).to_string(),
    );
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(
        serde_json::from_str::<Value>(&response.body).unwrap(),
        json!({"data":{"roster":{"edges":[{"node":{"id":"member-a","display":"Nia","media":[{"id":"media-a","src":"a.png","viewerLabel":"guest:owner-a"}],"treasury":{"id":"vault-a","credits":9}}}]}}})
    );
    let visibility = server
        .graphql("{media:__type(name:\"Media\"){fields{name}} hidden:__type(name:\"User\"){name}}");
    assert!(visibility["data"]["hidden"].is_null());
    assert!(
        !visibility["data"]["media"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "ownerCode")
    );

    let forged = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{forgeMedia{id src}}"}).to_string(),
    );
    assert_eq!(forged.status, 500);
    let forged_body: Value = serde_json::from_str(&forged.body).unwrap();
    assert_eq!(forged_body["data"], Value::Null);
    assert_eq!(forged_body["errors"][0]["message"], "Mock execution failed");
    server.stop("-TERM");
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let diagnostic = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|record| record["coordinate"] == "Commands.forgeMedia")
        .expect("forged media diagnostic");
    assert_eq!(diagnostic["cause"], "MOCK_RECORD_PROVENANCE");
    assert_eq!(diagnostic["module"], "resolvers/mutation/forgeMedia.luau");
    assert_eq!(diagnostic["line"], Value::Null);
    assert_eq!(diagnostic["column"], Value::Null);
    assert!(!stderr.contains("PRIVATE_SECOND_DOMAIN"));
}

fn build() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("domain.graphql"), PUBLIC).unwrap();
    assert_success(
        &directory,
        &["init", "--schema", "domain.graphql", "--config-dir", "mock"],
    );
    let root = directory.0.join("mock");
    fs::write(root.join("storage.graphql"), STORAGE).unwrap();
    let config = fs::read_to_string(root.join("mock.luau"))
        .unwrap()
        .replace("127.0.0.1:4000", "127.0.0.1:0");
    fs::write(root.join("mock.luau"), config).unwrap();
    assert_success(&directory, &["update", "--config-dir", "mock"]);
    fs::write(
        root.join("seeds/Member.jsonl"),
        "{\"id\":\"member-a\",\"display\":\"Nia\",\"media\":[{\"$ref\":{\"model\":\"Media\",\"key\":{\"id\":\"media-a\"}}}],\"treasuryCode\":\"vault-a\",\"rank\":7}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Media.jsonl"),
        "{\"id\":\"media-a\",\"src\":\"a.png\",\"ownerCode\":\"owner-a\"}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Treasury.jsonl"),
        "{\"id\":\"vault-a\",\"credits\":9}\n",
    )
    .unwrap();
    let schema = read_project_schema(&root).unwrap();
    let ir = schema.ir();
    for resolver in Layout::new(ir).unwrap().resolvers {
        let module =
            render_module(ir, resolver.script, resolver_body(&resolver.coordinate)).unwrap();
        fs::write(root.join(resolver.path.as_str()), module).unwrap();
    }
    directory
}

fn resolver_body(coordinate: &str) -> &'static str {
    match coordinate {
        "Directory.roster" => {
            r#"
local member = ctx.models["Member"]:find({
  where={field="rank",op="gte",value=1},
  orderBy={{field="rank",direction="desc"},{field="display",direction="asc"}}
})[1] or ctx.fail({code="NOT_FOUND"})
return {edges={{node=member}}}
"#
        }
        "Media.viewerLabel" => {
            r#"
if ctx.request == ctx.null then return "anonymous:" .. parent.ownerCode end
local request = ctx.request :: ChigiriHookRequest
for _, header in request.headers do
  if header.name == "x-viewer" then return header.value .. ":" .. parent.ownerCode end
end
return "anonymous:" .. parent.ownerCode
"#
        }
        "Commands.forgeMedia" => {
            "return {id='forged',src='hidden',ownerCode='PRIVATE_SECOND_DOMAIN'} :: any"
        }
        _ => panic!("missing renamed-domain resolver: {coordinate}"),
    }
}

fn assert_success(directory: &Directory, arguments: &[&str]) {
    let output = directory.command(arguments);
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
