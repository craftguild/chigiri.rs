--!strict
local hook: ChigiriRequestBeforeHook = {
    run = function(ctx: ChigiriRequestBeforeContext): ChigiriRequestBeforeResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
