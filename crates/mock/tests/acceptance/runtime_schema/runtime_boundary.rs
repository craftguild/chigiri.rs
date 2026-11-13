//! Verify nested-root permissions, abstract/model return values, and same-server recovery.

use super::{data, failure, init, ready};
use crate::support::{Server, with_seed};
use serde_json::json;

#[test]
fn rsm_08_nested_query_and_mutation_root_types_keep_parent_and_read_capability() {
    let _test_case = crate::support::test_case();
    let source = r#"
        schema { query: Read mutation: Write }
        type Item { id: ID! name: String! }
        input New { name: String! }
        type Read {
            root: Read!
            mutation: Write!
            note: String!
        }
        type Write {
            create(input: New!): Item!
            peek: Item
        }
    "#;
    let (_, config) = init(source);
    let config = ready(
        &config,
        &[
            ("Read.root", "return {}"),
            ("Read.mutation", "return {}"),
            (
                "Write.create",
                "if ctx.scope == 'mutation' then return ctx.models.Item:create({id='Item-' .. (ctx.models.Item:count({}) + 1), name=args.input.name}) else error('write requires mutation root') end",
            ),
            (
                "Read.note",
                "if parent == nil then return 'root' end return 'nested'",
            ),
            (
                "Write.peek",
                "return ctx.models.Item:find({limit=1})[1] or ctx.null",
            ),
        ],
    );
    let mut server = Server::start(&config);
    assert_eq!(
        data(&server, "{note root{note} mutation{peek{id}}}"),
        json!({"note":"root","root":{"note":"nested"},"mutation":{"peek":null}})
    );
    failure(
        &server,
        r#"{mutation{create(input:{name:"forbidden"}){id}}}"#,
    );
    assert_eq!(
        data(
            &server,
            r#"mutation{create(input:{name:"allowed"}){id} peek{id}}"#
        ),
        json!({"create":{"id":"Item-1"},"peek":{"id":"Item-1"}})
    );
    assert_eq!(
        data(&server, "{mutation{peek{id name}}}"),
        json!({"mutation":{"peek":{"id":"Item-1","name":"allowed"}}})
    );
    server.stop("-TERM");
}

#[test]
fn rsm_07_14_invalid_abstract_record_and_scalar_returns_fail_then_same_server_recovers() {
    let _test_case = crate::support::test_case();
    let (_, config) = init(
        r#"
        scalar Code @representation(type: STRING, minBytes: 2)
        type Item { id: ID! name: String! }
        type Packet { title: String! }
        union Result = Item | Packet
        type Query {
            items: [Item!]!
            result: Result
            code: Code!
        }
    "#,
    );
    let config = with_seed(&config, &json!({"Item":[{"id":"Item-1","name":"stored"}]}));
    for body in [
        "return {__typename='Item', id='Item-1', name='changed'} :: any",
        "return {__typename='Item', id='fake', name='stored'} :: any",
        "return {__typename='Unknown', title='x'} :: any",
        "return {title='missing discriminator'} :: any",
        "return {__typename='Packet', title='x', extra=true} :: any",
    ] {
        let mut server = Server::start(&ready(
            &config,
            &[
                ("Query.items", "return ctx.models.Item:find({})"),
                ("Query.result", body),
                ("Query.code", "return 'x'"),
            ],
        ));
        let result = failure(&server, "{result{__typename}} ");
        assert_eq!(result["data"]["result"], serde_json::Value::Null);
        assert_eq!(
            result["errors"][0]["extensions"]["code"],
            "MOCK_EXECUTION_FAILED"
        );
        let result = failure(&server, "{code}");
        assert_eq!(result["data"], serde_json::Value::Null);
        assert_eq!(
            data(&server, "{items{id name}}"),
            json!({"items":[{"id":"Item-1","name":"stored"}]})
        );
        server.stop("-TERM");
    }
}
