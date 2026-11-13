--!strict
local resolver: ChigiriQueryTodosResolver = {
    resolve = function(
        ctx: ChigiriQueryTodosContext,
        args: ChigiriQueryTodosArgs
    ): ChigiriQueryTodosResult
        -- Signature: todos()
        return ctx.models.Todo:find({})
    end,
}
return resolver
