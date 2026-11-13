//! Executable reference examples. User and Post are illustrative schema names.

use super::Effect;

pub(crate) struct ExampleSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub effect: Effect,
    pub body: &'static str,
}

pub(crate) const EXAMPLES: &[ExampleSpec] = &[
    ExampleSpec {
        id: "query.operators",
        title: "Evaluate every query operator at representative boundaries.",
        effect: Effect::Read,
        body: r#"local model = ctx.models["User"]
return model:count({where = {field = "rank", op = "eq", value = 1}}) == 1
    and model:count({where = {field = "rank", op = "ne", value = 1}}) == 1
    and model:count({where = {field = "rank", op = "in", value = {1, 2}}}) == 2
    and model:count({where = {field = "rank", op = "lt", value = 2}}) == 1
    and model:count({where = {field = "rank", op = "lte", value = 2}}) == 2
    and model:count({where = {field = "rank", op = "gt", value = 1}}) == 1
    and model:count({where = {field = "rank", op = "gte", value = 1}}) == 2
    and model:count({where = {field = "name", op = "contains", value = "Ali"}}) == 1
    and model:count({where = {field = "note", op = "isNull", value = true}}) == 1"#,
    },
    ExampleSpec {
        id: "find.records",
        title: "Read detached record copies in deterministic order.",
        effect: Effect::Read,
        body: "local rows = ctx.models[\"User\"]:find({})\nreturn #rows",
    },
    ExampleSpec {
        id: "count.records",
        title: "Count matching records without materializing a page.",
        effect: Effect::Read,
        body: "return ctx.models[\"User\"]:count({})",
    },
    ExampleSpec {
        id: "create.record",
        title: "Create and persist one validated record.",
        effect: Effect::Write,
        body: "local row = ctx.models[\"User\"]:create({name = \"Chika\", rank = 3})\nreturn row.id",
    },
    ExampleSpec {
        id: "update.patch",
        title: "Patch selected fields and assign an explicit null.",
        effect: Effect::Write,
        body: "local rows = ctx.models[\"User\"]:update({where = {field = \"id\", op = \"eq\", value = \"1\"}}, {name = \"Renamed\", note = ctx.null})\nreturn rows[1].name",
    },
    ExampleSpec {
        id: "delete.record",
        title: "Delete the unreferenced record.",
        effect: Effect::Write,
        body: "return ctx.models[\"User\"]:delete({where = {field = \"id\", op = \"eq\", value = \"2\"}})",
    },
    ExampleSpec {
        id: "connect.owner",
        title: "Connect selected records through a declared GraphQL relation.",
        effect: Effect::Write,
        body: "local source: ChigiriPostFindOptions = {where = {field = \"id\", op = \"eq\", value = \"1\"}}\nlocal target: ChigiriUserFindOptions = {where = {field = \"id\", op = \"eq\", value = \"1\"}}\nlocal rows = ctx.models[\"Post\"]:connect(source, \"author\", target)\nreturn rows[1].authorId or ctx.null",
    },
    ExampleSpec {
        id: "disconnect.owner",
        title: "Disconnect selected records from a declared relation.",
        effect: Effect::Write,
        body: "local source: ChigiriPostFindOptions = {where = {field = \"id\", op = \"eq\", value = \"1\"}}\nlocal target: ChigiriUserFindOptions = {where = {field = \"id\", op = \"eq\", value = \"1\"}}\nctx.models[\"Post\"]:connect(source, \"author\", target)\nlocal rows = ctx.models[\"Post\"]:disconnect(source, \"author\", target)\nreturn rows[1].authorId == ctx.null",
    },
    ExampleSpec {
        id: "query.predicates",
        title: "Combine predicates with closed all/any groups.",
        effect: Effect::Read,
        body: "return ctx.models[\"User\"]:count({where = {all = {{field = \"rank\", op = \"gte\", value = 1}, {any = {{field = \"name\", op = \"contains\", value = \"li\"}, {field = \"rank\", op = \"in\", value = {2}}}}}}})",
    },
    ExampleSpec {
        id: "query.page",
        title: "Sort records before applying offset and limit.",
        effect: Effect::Read,
        body: "local order: {{field: \"rank\", direction: \"desc\"}} = {{field = \"rank\", direction = \"desc\"}}\nlocal rows = ctx.models[\"User\"]:find({orderBy = order, offset = 1, limit = 1})\nreturn rows[1].id",
    },
    ExampleSpec {
        id: "query.null",
        title: "Match null and omitted fields with isNull.",
        effect: Effect::Read,
        body: "return ctx.models[\"User\"]:count({where = {field = \"note\", op = \"isNull\", value = true}})",
    },
    ExampleSpec {
        id: "transaction.candidate",
        title: "Read earlier writes from the same mutation candidate state.",
        effect: Effect::Write,
        body: "local before = ctx.models[\"User\"]:count({})\nctx.models[\"User\"]:create({name = \"New\", rank = 4})\nreturn ctx.models[\"User\"]:count({}) - before",
    },
    ExampleSpec {
        id: "transaction.caught",
        title: "Prevent commit when a protected call catches a host failure.",
        effect: Effect::Write,
        body: "ctx.models[\"User\"]:create({name = \"Rolled back\", rank = 4})\nlocal model = ctx.models[\"User\"] :: any\npcall(function() model:count({limit = 1}) end)\nreturn true",
    },
];
