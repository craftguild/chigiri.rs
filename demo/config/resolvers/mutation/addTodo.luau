--!strict
local resolver: ChigiriMutationAddTodoResolver = {
    resolve = function(
        ctx: ChigiriMutationAddTodoContext,
        args: ChigiriMutationAddTodoArgs
    ): ChigiriMutationAddTodoResult
        -- Signature: addTodo(title)
        return ctx.models.Todo:create({ title = args.title, completed = false })
    end,
}
return resolver
