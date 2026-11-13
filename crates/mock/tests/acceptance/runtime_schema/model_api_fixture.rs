//! Provide project-domain CRUD and DTO resolver bodies for model API rollback tests.

use super::{PROJECTS, projects_source, ready};

pub(super) fn config() -> String {
    let sdl = format!(
        "{PROJECTS}\n{}",
        include_str!("../../../fixtures/model-api.graphql")
    );
    let config = projects_source(&sdl);
    let count = r#"
if args.status == nil or args.status == ctx.null then return ctx.models.Member:count({}) end
return ctx.models.Member:count({where={field="status", op="eq", value=args.status :: ChigiriStatusValue}})
"#;
    let find = r#"
local order: {{field:"code", direction:"asc"}} = {{field="code", direction="asc"}}
return ctx.models.Member:find({where={field="code",op="contains",value=args.needle},
    orderBy=order,offset=args.offset,limit=args.limit})
"#;
    let link = write(
        "local source: ChigiriProjectFindOptions = {where={field='id',op='eq',value=args.id}} local target: ChigiriMemberFindOptions = {where={field='id',op='eq',value=args.memberId}} return ctx.models.Project:connect(source, 'members', target)[1]",
    );
    let unlink = write(
        "local source: ChigiriProjectFindOptions = {where={field='id',op='eq',value=args.id}} local target: ChigiriMemberFindOptions = {where={field='id',op='eq',value=args.memberId}} return ctx.models.Project:disconnect(source, 'members', target)[1]",
    );
    let failure = write(&format!(
        r#"
ctx.models.Member:create({{name="rolled",status="OPEN",code="RR"}})
ctx.models.Project:create({{name="rolled",memberIds={{}}}})
local source: ChigiriProjectFindOptions = {{where={{field="id",op="eq",value=args.id}}}}
local target: ChigiriMemberFindOptions = {{where={{field="id",op="eq",value=args.memberId}}}}
ctx.models.Project:connect(source,"members",target)
ctx.models.Project:update(source,{{name="dirty"}})
local model = ctx.models.Member :: any
if args.mode == "host" then
    pcall(function() model:find({{offset=-1}}) end)
elseif args.mode == "return" then
    return "invalid boolean" :: any
elseif args.mode == "script" then
    error("intentional rollback")
elseif args.mode == "work" then
    local needle = "{}"
    pcall(function()
        for _ = 1, 200 do model:count({{where={{field="code",op="contains",value=needle}}}}) end
    end)
elseif args.mode == "loop" then
    while true do end
end
return true
"#,
        "x".repeat(4096)
    ));
    ready(
        &config,
        &[
            ("Query.memberCount", count),
            ("Query.projectCount", "return ctx.models.Project:count({})"),
            ("Query.searchMembers", find),
            (
                "Mutation.addMember",
                "if ctx.scope == 'mutation' then local id = 'Member-3' if args.input.name == 'Ada' then id = 'Member-1' elseif args.input.name == 'Ben' then id = 'Member-2' end if args.input.managerId ~= nil then return ctx.models.Member:create({id=id,name=args.input.name,status='OPEN',code=args.input.code,managerId=args.input.managerId}) end return ctx.models.Member:create({id=id,name=args.input.name,status='OPEN',code=args.input.code}) else error('write requires mutation root') end",
            ),
            ("Query.root", "return {}"),
            (
                "Query.forbidden",
                "local model = ctx.models.Project :: any model:connect({}, 'members', {}) return true",
            ),
            ("Mutation.linkProject", &link),
            ("Mutation.unlinkProject", &unlink),
            ("Mutation.failProject", &failure),
            (
                "Mutation.unchecked",
                "local model = ctx.models.Project :: any model:connect({where={field='id',op='eq',value=args.id}}, 'members', {where={field='id',op='eq',value=args.memberId}}) return true",
            ),
        ],
    )
}

fn write(body: &str) -> String {
    format!(
        "if ctx.scope == 'mutation' then\n{body}\nelse\nerror('write requires mutation root')\nend"
    )
}
