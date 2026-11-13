--!strict
local hook: ChigiriFieldBeforeHook = {
    run = function(ctx: ChigiriFieldBeforeContext): ChigiriFieldBeforeResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
