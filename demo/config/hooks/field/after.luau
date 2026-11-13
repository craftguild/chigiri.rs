--!strict
local hook: ChigiriFieldAfterHook = {
    run = function(ctx: ChigiriFieldAfterContext): ChigiriFieldAfterResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
