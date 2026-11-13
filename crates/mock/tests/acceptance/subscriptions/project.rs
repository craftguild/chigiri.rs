//! Build a checked fresh project with short typed subscription scripts.

use std::fs;

use chigiri_mock::ir::ScriptRole;
use chigiri_mock::luau::{render_module, render_subscription_module};
use chigiri_mock::project::{Layout, read_project_schema};

use crate::support::Directory;

const SCHEMA: &str = r#"
schema { query: Query mutation: Mutation subscription: Subscription }
type Query { count: Int! seed: Item! }
type Mutation {
  publish(topic: String! = "events", id: ID!, value: Int!, padding: String! = ""): Item!
  publishPair(first: ID!, second: ID!): Int!
  publishBurst(prefix: String!, padding: String!): Int!
  rollback(id: ID!): Int!
  mutateSeed(value: Int!): Item!
}
type Subscription { changed(topic: String!): Item! }
type Item {
  id: ID!
  value: Int!
  padding: String!
  relatedId: ID!
  related: Related! @relation(fields:["relatedId"], references:["id"])
  explode(mode: String = ""): Int!
}
type Related { id: ID! label: String! }
"#;

pub(super) fn checked() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), SCHEMA).unwrap();
    let init = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let root = directory.0.join("mock");
    let source = read_project_schema(&root).unwrap();
    let ir = source.ir();
    for resolver in Layout::new(ir).unwrap().resolvers {
        let body = body(&resolver.coordinate);
        let module = if matches!(
            ir.scripts()[resolver.script.0].role,
            ScriptRole::SubscriptionRoot { .. }
        ) {
            render_subscription_module(ir, resolver.script, body, None).unwrap()
        } else {
            render_module(ir, resolver.script, body).unwrap()
        };
        fs::write(root.join(resolver.path.as_str()), module).unwrap();
    }
    fs::write(
        root.join("seeds/Related.jsonl"),
        "{\"id\":\"related-seed\",\"label\":\"seed-baseline\"}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Item.jsonl"),
        "{\"id\":\"seed\",\"value\":7,\"padding\":\"\",\"relatedId\":\"related-seed\"}\n",
    )
    .unwrap();
    let config = fs::read_to_string(root.join("mock.luau")).unwrap();
    fs::write(
        root.join("mock.luau"),
        config.replace("127.0.0.1:4000", "127.0.0.1:0"),
    )
    .unwrap();
    let check = directory.command(&["check", "--config-dir", "mock"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    directory
}

pub(super) fn checked_sequence(looping: bool) -> Directory {
    let directory = checked();
    let root = directory.0.join("mock");
    let source = read_project_schema(&root).unwrap();
    let ir = source.ir();
    let resolver = Layout::new(ir)
        .unwrap()
        .resolvers
        .into_iter()
        .find(|resolver| resolver.coordinate == "Subscription.changed")
        .unwrap();
    let body = format!(
        "return ctx.generator.sequence.Item({{values=ctx.models.Item:find({{where={{field='id',op='eq',value='seed'}}}}),interval_ms=20,loop={looping}}})"
    );
    fs::write(
        root.join(resolver.path.as_str()),
        render_subscription_module(ir, resolver.script, &body, None).unwrap(),
    )
    .unwrap();
    let checked = directory.command(&["check", "--config-dir", "mock"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    directory
}

fn body(coordinate: &str) -> &'static str {
    match coordinate {
        "Query.count" => "return ctx.models.Item:count({})",
        "Query.seed" => {
            "return ctx.models.Item:find({where={field='id',op='eq',value='seed'},limit=1})[1]"
        }
        "Mutation.publish" => {
            r#"
local relatedId = "related-" .. args.id
ctx.models.Related:create({id=relatedId,label="captured-" .. args.id})
local row = ctx.models.Item:create({id=args.id,value=args.value,padding=args.padding,relatedId=relatedId})
ctx.pubsub.publish.Item({topic=args.topic,payload=row})
local current = ctx.models.Item:update({where={field="id",op="eq",value=args.id}},{value=args.value+100})[1]
ctx.models.Related:update({where={field="id",op="eq",value=relatedId}},{label="current-" .. args.id})
return current
"#
        }
        "Mutation.publishPair" => {
            r#"
local firstRelated = "related-" .. args.first
ctx.models.Related:create({id=firstRelated,label="captured-" .. args.first})
local first = ctx.models.Item:create({id=args.first,value=1,padding="",relatedId=firstRelated})
ctx.pubsub.publish.Item({topic="events",payload=first})
ctx.models.Item:update({where={field="id",op="eq",value=args.first}},{value=101})
ctx.models.Related:update({where={field="id",op="eq",value=firstRelated}},{label="current-" .. args.first})
local secondRelated = "related-" .. args.second
ctx.models.Related:create({id=secondRelated,label="captured-" .. args.second})
local second = ctx.models.Item:create({id=args.second,value=2,padding="",relatedId=secondRelated})
ctx.pubsub.publish.Item({topic="events",payload=second})
ctx.models.Item:update({where={field="id",op="eq",value=args.second}},{value=102})
ctx.models.Related:update({where={field="id",op="eq",value=secondRelated}},{label="current-" .. args.second})
return 2
"#
        }
        "Mutation.publishBurst" => {
            r#"
local relatedId = "related-" .. args.prefix
ctx.models.Related:create({id=relatedId,label="source-pressure"})
for index = 1, 8 do
    local id = args.prefix .. index
    local row = ctx.models.Item:create({id=id,value=index,padding=args.padding,relatedId=relatedId})
    ctx.pubsub.publish.Item({topic="events",payload=row})
end
return 8
"#
        }
        "Mutation.rollback" => {
            r#"
local relatedId = "related-" .. args.id
ctx.models.Related:create({id=relatedId,label="captured-" .. args.id})
local row = ctx.models.Item:create({id=args.id,value=99,padding="",relatedId=relatedId})
ctx.pubsub.publish.Item({topic="events",payload=row})
error("private rollback failure")
"#
        }
        "Mutation.mutateSeed" => {
            r#"
ctx.models.Related:update({where={field="id",op="eq",value="related-seed"}},{label="seed-mutated"})
return ctx.models.Item:update({where={field="id",op="eq",value="seed"}},{value=args.value})[1]
"#
        }
        "Subscription.changed" => "return ctx.pubsub.subscribe.Item({topic=args.topic})",
        "Item.explode" => {
            "if parent.value == 13 then error(\"private event failure\") end return parent.value"
        }
        _ => panic!("missing subscription fixture resolver: {coordinate}"),
    }
}
