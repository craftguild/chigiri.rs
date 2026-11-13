//! Verify idless models, duplicates, full selection, explicit M:N relations, and regex rollback over real HTTP.

use super::super::{data, failure, init, ready};
use crate::support::{Server, with_seed};
use serde_json::json;

#[test]
fn sd_idless_duplicates_unmarked_values_bulk_selection_and_implicit_edges_over_http() {
    let _case = crate::support::test_case();
    let (_, config) = init(
        r#"
        type Entry { rank:Int! nodes:[Other!]! }
        type Other { entries:[Entry!]! }
        type PageInfo { count:Int! }
        type A { id:Int! bs:[B!]! @relation }
        type B { id:Int! as:[A!]! @relation }
        type Query { rows(offset:Int! = 0, limit:Int! = 1000):[Entry!]! info:PageInfo! a:[A!]! b:[B!]! otherCount:Int! }
        type Mutation { patch(offset:Int!,limit:Int!):[Entry!]! invalid:Boolean wipe:Int! link:Int! reconnect:Int! }
    "#,
    );
    let config = ready(
        &config,
        &[
            (
                "Query.rows",
                "return ctx.models.Entry:find({offset=args.offset,limit=args.limit})",
            ),
            ("Query.info", "return {count=ctx.models.Entry:count({})}"),
            ("Query.a", "return ctx.models.A:find({})"),
            ("Query.b", "return ctx.models.B:find({})"),
            ("Query.otherCount", "return ctx.models.Other:count({})"),
            (
                "Mutation.patch",
                "return ctx.models.Entry:update({offset=args.offset,limit=args.limit},{rank=999})",
            ),
            (
                "Mutation.invalid",
                "ctx.models.Other:create({entries={}}) ctx.models.Entry:update({limit=1},{rank=77}) local model = ctx.models.Entry :: any pcall(function() model:update({limit=0},{unknown=1}) end) return true",
            ),
            ("Mutation.wipe", "return ctx.models.Entry:delete({})"),
            (
                "Mutation.link",
                "local source: ChigiriAFindOptions = {} local order: {{field: \"id\", direction: \"desc\"}} = {{field='id',direction='desc'}} local target: ChigiriBFindOptions = {orderBy=order} return #ctx.models.A:connect(source,'bs',target)",
            ),
            (
                "Mutation.reconnect",
                "local source: ChigiriBFindOptions = {where={field='id',op='eq',value=2}} local target: ChigiriAFindOptions = {where={field='id',op='eq',value=1}} ctx.models.B:disconnect(source,'as',target) return #ctx.models.B:connect(source,'as',target)",
            ),
        ],
    );
    let rows = (0..101)
        .map(|index| json!({"rank":if index < 2 {0} else {index},"nodes":[{"entries":[]}]}))
        .collect::<Vec<_>>();
    let config = with_seed(
        &config,
        &json!({"Entry":rows,"A":[{"id":1},{"id":2}],"B":[{"id":1},{"id":2},{"id":3,"as":[1]}]}),
    );
    let mut server = Server::offline(&config);
    let initial = data(
        &server,
        "{rows{rank nodes{entries{rank}}} info{count} otherCount}",
    );
    assert_eq!(initial["rows"].as_array().unwrap().len(), 101);
    assert_eq!(initial["rows"][0], initial["rows"][1]);
    assert_eq!(initial["rows"][0]["nodes"], json!([{"entries":[]}]));
    assert_eq!(initial["info"], json!({"count":101}));
    assert_eq!(initial["otherCount"], 0);
    assert_eq!(
        data(
            &server,
            "{rows(offset:100){rank} empty:rows(limit:0){rank}}"
        ),
        json!({"rows":[{"rank":100}],"empty":[]})
    );
    let failed = failure(&server, "mutation{invalid}");
    assert!(failed["data"].is_null());
    assert_eq!(
        data(
            &server,
            "{rows{rank nodes{entries{rank}}} info{count} otherCount}"
        ),
        initial
    );
    assert_eq!(
        data(&server, "mutation{patch(offset:1,limit:1){rank}}"),
        json!({"patch":[{"rank":999}]})
    );
    assert_eq!(
        data(&server, "{rows(limit:3){rank}}"),
        json!({"rows":[{"rank":0},{"rank":999},{"rank":2}]})
    );
    assert_eq!(data(&server, "mutation{link}"), json!({"link":2}));
    let edges = data(&server, "{a{id bs{id}} b{id as{id}}}");
    for a in edges["a"].as_array().unwrap() {
        assert_eq!(a["bs"], json!([{"id":3},{"id":2},{"id":1}]));
    }
    assert_eq!(edges["b"][2]["as"], json!([{"id":1},{"id":2}]));
    assert_eq!(data(&server, "mutation{reconnect}"), json!({"reconnect":1}));
    assert_eq!(
        data(&server, "{a{id bs{id}}}")["a"][0]["bs"],
        json!([{"id":3},{"id":1},{"id":2}])
    );
    assert_eq!(data(&server, "mutation{wipe}"), json!({"wipe":101}));
    assert_eq!(
        data(&server, "{rows{rank} info{count}}"),
        json!({"rows":[],"info":{"count":0}})
    );
    server.stop("-TERM");
}

#[test]
fn sd_regex_aggregate_failure_cannot_be_caught_and_same_binary_server_recovers() {
    let _case = crate::support::test_case();
    let (_, config) = init(
        r#"
        type A{words:[String!]! @constraint(pattern:"^$")}
        type B{words:[String!]! @constraint(pattern:"^$")}
        type Query{counts:[Int!]!} type Mutation{write(fail:Boolean!):Int!}
    "#,
    );
    let config = ready(
        &config,
        &[
            (
                "Query.counts",
                "return {ctx.models.A:count({}),ctx.models.B:count({})}",
            ),
            (
                "Mutation.write",
                r#"
            local words: {string} = {}
            if args.fail then for i=1,1000 do words[i] = '' end else words[1] = '' end
            ctx.models.A:create({words=words})
            ctx.models.B:create({words=words})
            if args.fail then
                ctx.models.A:create({words=words})
                ctx.models.B:create({words=words})
                pcall(function() ctx.models.A:create({words=words}) end)
            end
            return 2
        "#,
            ),
        ],
    );
    let mut server = Server::offline(&config);
    let failed = failure(&server, "mutation{write(fail:true)}");
    assert!(failed["data"].is_null());
    assert_eq!(
        failed["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(data(&server, "{counts}"), json!({"counts":[0,0]}));
    assert_eq!(
        data(&server, "mutation{write(fail:false)}"),
        json!({"write":2})
    );
    assert_eq!(data(&server, "{counts}"), json!({"counts":[1,1]}));
    server.stop("-TERM");
}
