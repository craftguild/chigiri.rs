--!strict
local hook: ChigiriOperationBeforeHook = {
    run = function(ctx: ChigiriOperationBeforeContext): ChigiriOperationBeforeResult
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
