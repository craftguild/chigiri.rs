//! Build a maintained renamed-root consumer example through ordinary init/codegen.

use std::fs;

use chigiri_mock::luau::{render_consumer_module, render_module, render_subscription_module};
use chigiri_mock::project::{Layout, read_project_schema};

use crate::support::Directory;

const SDL: &str = r#"
schema {query:Read mutation:Write subscription:Signals}
type Item {id:ID! value:Int!}
type Audit {id:ID! value:Int! depth:Int! counter:Int}
type Read {items:[Item!]! audits:[Audit!]!}
type Write {publish(id:ID!,value:Int!,topic:String!):Int!}
type Signals {changed(required:ID!):Item!}
"#;

const EVENTS: &str = r#"
config.clock = {mode='fixed',now=123}
config.state = {fields={marker={kind='string'}},initial={marker='initial'}}
config.hooks.request.before = true
config.events = {
    consumers = {
        source = {field='changed',enabled=true},
        audit = {field='changed',enabled=true},
        process = {field='changed',enabled=true},
        disabled = {field='changed',enabled=false},
    },
}
"#;

const SOURCE: &str = r#"
assert(ctx.scope=='consumer_setup' and ctx.request==ctx.null and ctx.state.marker=='initial')
assert(os.time()==123)
return ctx.generator.sequence.Item({values=ctx.models.Item:find({}),interval_ms=100,loop=true})
"#;

const SOURCE_CONSUME: &str = r#"
assert(ctx.request==ctx.null and ctx.state.marker=='initial' and os.time()==123)
assert(ctx.event.topic==ctx.null and ctx.event.depth==0 and ctx.event.droppedTotal==ctx.null)
if ctx.models.Audit:count({where={field='id',op='eq',value='source'}})==0 then
    ctx.models.Audit:create({id='source',value=event.value,depth=ctx.event.depth})
end
ctx.pubsub.publish.Item({topic='out',payload=event})
"#;

const AUDIT: &str = r#"
assert(ctx.request==ctx.null and ctx.state.marker=='initial' and os.time()==123)
local topic = ctx.event.topic
assert(type(topic)=='string' and ctx.event.droppedTotal~=ctx.null)
if ctx.models.Audit:count({where={field='id',op='eq',value=topic}})==0 then
    ctx.models.Audit:create({id=topic,value=event.value,depth=ctx.event.depth,counter=ctx.event.droppedTotal})
end
"#;

const PROCESS: &str = r#"
assert(ctx.request==ctx.null and ctx.state.marker=='initial' and os.time()==123)
ctx.models.Audit:create({id='candidate-'..event.id,value=event.value,depth=ctx.event.depth})
ctx.pubsub.publish.Item({topic='child-'..event.id,payload=event})
if event.value==13 then ctx.fail({code='REJECTED'}) end
ctx.log('info','processed',{id=event.id})
"#;

pub(super) fn checked() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), SDL).unwrap();
    success(
        &directory,
        &["init", "--schema", "schema.graphql", "--config-dir", "mock"],
    );
    let root = directory.0.join("mock");
    let config = fs::read_to_string(root.join("mock.luau")).unwrap();
    fs::write(
        root.join("mock.luau"),
        config
            .replace("127.0.0.1:4000", "127.0.0.1:0")
            .replace("return config", &format!("{EVENTS}\nreturn config")),
    )
    .unwrap();
    success(&directory, &["codegen", "--config-dir", "mock"]);
    let schema = read_project_schema(&root).unwrap();
    let ir = schema.ir();
    for resolver in Layout::new(ir).unwrap().resolvers {
        let body = match resolver.coordinate.as_str() {
            "Read.items" => "return ctx.models.Item:find({})",
            "Read.audits" => "return ctx.models.Audit:find({})",
            "Write.publish" => {
                "local item=ctx.models.Item:create({id=args.id,value=args.value}) ctx.pubsub.publish.Item({topic=args.topic,payload=item}) return 1"
            }
            "Signals.changed" => "return ctx.pubsub.subscribe.Item({topic='out'})",
            coordinate => panic!("unexpected resolver: {coordinate}"),
        };
        let module = if ir.scripts()[resolver.script.0].role.is_subscription_root() {
            render_subscription_module(ir, resolver.script, body, None).unwrap()
        } else {
            render_module(ir, resolver.script, body).unwrap()
        };
        fs::write(root.join(resolver.path.as_str()), module).unwrap();
    }
    for registration in schema.settings().events.resolve(ir).unwrap() {
        let (subscribe, consume) = match registration.name() {
            "source" => (SOURCE, SOURCE_CONSUME),
            "audit" => (
                "return ctx.pubsub.subscribe.Item({all_topics=true,overflow='drop_oldest'})",
                AUDIT,
            ),
            "process" => ("return ctx.pubsub.subscribe.Item({topic='input'})", PROCESS),
            "disabled" => (
                "error('disabled setup must not run')",
                "error('disabled consume must not run')",
            ),
            name => panic!("unexpected registration: {name}"),
        };
        fs::write(
            root.join(registration.path().as_str()),
            render_consumer_module(ir, registration.field(), subscribe, consume).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        root.join("seeds/Item.jsonl"),
        "{\"id\":\"clock\",\"value\":7}\n",
    )
    .unwrap();
    fs::write(
        root.join("hooks/request/before.luau"),
        chigiri_mock::luau::render_hook_module(
            chigiri_mock::hooks::HookSlot::RequestBefore,
            "return {statePatch={marker='publisher'}}",
        )
        .unwrap(),
    )
    .unwrap();
    success(
        &directory,
        &[
            "check",
            "--config-dir",
            "mock",
            "--target",
            "consumers/process.luau",
            "--format",
            "json",
        ],
    );
    success(&directory, &["check", "--config-dir", "mock"]);
    directory
}

pub(super) fn success(directory: &Directory, arguments: &[&str]) -> Vec<u8> {
    let result = directory.command(arguments);
    assert!(
        result.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}
