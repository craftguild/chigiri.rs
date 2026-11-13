--!strict
local resolver: ChigiriMutationAddTodoResolver = {
    resolve = function(
        ctx: ChigiriMutationAddTodoContext,
        args: ChigiriMutationAddTodoArgs
    ): ChigiriMutationAddTodoResult
        -- This user-owned implementation must survive the format migration.
        -- Signature: addTodo(title)
        return ctx.models.Todo:create({ title = args.title, completed = false })
    end,
}
return resolver
