//! Blog-fixture operations with deterministic IDs and create-before-delete sequencing.

use crate::ir::MockIr;
use crate::luau::ScriptCandidate;

pub(super) const SDL: &str = include_str!("../../fixtures/blog.graphql");

const RESOLVERS: &[(&str, &str)] = &[
    (
        "Query.getUsers",
        "return ctx.models.User:find({limit=args.limit})",
    ),
    (
        "Query.findPost",
        r#"
        local filters: {ChigiriPostWhere} = {}
        local id = args.id
        local authorId = args.authorId
        if type(id) == "string" then
            local filter: ChigiriPostWhere = {field="id", op="eq", value=id}
            filters[#filters + 1] = filter
        end
        if type(authorId) == "string" then
            local filter: ChigiriPostWhere = {field="authorId", op="eq", value=authorId}
            filters[#filters + 1] = filter
        end
        return ctx.models.Post:find({where={all=filters}, limit=args.limit})
    "#,
    ),
    (
        "Mutation.createUser",
        r#"
        local id = 'User-' .. (ctx.models.User:count({}) + 1)
        return ctx.models.User:create({id=id, name=args.input.name})
    "#,
    ),
    (
        "Mutation.createPost",
        r#"
        local id = 'Post-' .. (ctx.models.Post:count({}) + 1)
        return ctx.models.Post:create({id=id, title=args.input.title, body=args.input.body, authorId=args.input.authorId})
    "#,
    ),
    (
        "Mutation.updatePost",
        r#"
        return ctx.models.Post:update({where={field='id', op='eq', value=args.id}}, args.input)[1]
    "#,
    ),
    (
        "Mutation.deletePost",
        r#"
        return ctx.models.Post:delete({where={field='id', op='eq', value=args.id}}) > 0
    "#,
    ),
];

pub(super) fn candidates(sdl: &str, ir: &MockIr, edits: &[(&str, &str)]) -> Vec<ScriptCandidate> {
    let mut bodies = Vec::new();
    if sdl == SDL {
        bodies.extend_from_slice(RESOLVERS);
    }
    bodies.extend_from_slice(edits);
    super::tests::candidates(ir, &bodies)
}
