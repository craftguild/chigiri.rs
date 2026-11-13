--!strict
local hook: ChigiriOperationAfterHook = {
    run = function(ctx: ChigiriOperationAfterContext): ChigiriOperationAfterResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
