//! Verify types, fields, and roots renamed around Luau reserved words work in the same binary.

use super::{data, init, ready};
use crate::support::{Server, seed};
use serde_json::json;

#[test]
fn rsm_05_06_18_19_renamed_types_fields_and_explicit_roots_work_offline() {
    let _test_case = crate::support::test_case();
    let renamed = include_str!("../../../fixtures/blog.graphql")
        .replace("getUsers", "while")
        .replace("User", "end")
        .replace("Post", "local")
        .replace("name", "function")
        .replace("author", "owner")
        .replace("posts", "entries")
        .replace("Query", "Lookup")
        .replace("Mutation", "Changes");
    let (_, scaffold) = init(&format!(
        "schema {{ query: Lookup mutation: Changes }}\n{renamed}"
    ));
    let config = ready(
        &scaffold,
        &[
            (
                "Lookup.while",
                "return ctx.models['end']:find({limit=args.limit})",
            ),
            (
                "Lookup.findlocal",
                "local owner = args.ownerId if type(owner) == 'string' then return ctx.models['local']:find({where={field='ownerId',op='eq',value=owner},limit=args.limit}) end return ctx.models['local']:find({limit=args.limit})",
            ),
            (
                "Changes.createend",
                "return ctx.models['end']:create({id='end-' .. (ctx.models['end']:count({}) + 1), ['function']=args.input['function']})",
            ),
            (
                "Changes.createlocal",
                "return ctx.models['local']:create({id='local-1', title=args.input.title, body=args.input.body, ownerId=args.input.ownerId})",
            ),
            (
                "Changes.updatelocal",
                "return ctx.models['local']:update({where={field='id',op='eq',value=args.id}}, {title=args.input.title,body=args.input.body,ownerId=args.input.ownerId})[1]",
            ),
            (
                "Changes.deletelocal",
                "return ctx.models['local']:delete({where={field='id',op='eq',value=args.id}}) > 0",
            ),
        ],
    );
    assert_eq!(seed(&config), json!({"end":[],"local":[]}));
    let mut server = Server::offline(&config);
    assert_eq!(
        data(
            &server,
            r#"mutation {
        a:createend(input:{function:"one"}) {__typename id function}
        b:createend(input:{function:"two"}) {id}
        p:createlocal(input:{title:"renamed",body:"text",ownerId:"end-1"}) {__typename id owner{id}}
    }"#
        ),
        json!({"a":{"__typename":"end","id":"end-1","function":"one"},"b":{"id":"end-2"},"p":{"__typename":"local","id":"local-1","owner":{"id":"end-1"}}})
    );
    assert_eq!(
        data(
            &server,
            r#"mutation {updatelocal(id:"local-1",input:{ownerId:"end-2"}) {id owner{id entries{id}}}}"#
        ),
        json!({"updatelocal":{"id":"local-1","owner":{"id":"end-2","entries":[{"id":"local-1"}]}}})
    );
    assert_eq!(
        data(
            &server,
            "{while(limit:1){function entries{id}} findlocal(ownerId:\"end-2\"){id}}"
        ),
        json!({"while":[{"function":"one","entries":[]}],"findlocal":[{"id":"local-1"}]})
    );
    assert_eq!(
        data(&server, r#"mutation {deletelocal(id:"local-1")}"#),
        json!({"deletelocal":true})
    );
    assert_eq!(data(&server, "{findlocal{id}}"), json!({"findlocal":[]}));
    server.stop("-TERM");
}
