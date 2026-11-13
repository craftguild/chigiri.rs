--!strict
local resolver: ChigiriQueryTodosResolver = {
    resolve = function(
        ctx: ChigiriQueryTodosContext,
        args: ChigiriQueryTodosArgs
    ): ChigiriQueryTodosResult
        -- This user-owned implementation must survive the format migration.
        -- Signature: todos()
        return ctx.models.Todo:find({})
    end,
}
return resolver
