--!strict
local hook: ChigiriRequestAfterHook = {
    run = function(ctx: ChigiriRequestAfterContext): ChigiriRequestAfterResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
