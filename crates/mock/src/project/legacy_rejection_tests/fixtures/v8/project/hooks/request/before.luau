--!strict
local hook: ChigiriRequestBeforeHook = {
    run = function(ctx: ChigiriRequestBeforeContext): ChigiriRequestBeforeResult
        -- This user-owned hook edit must survive even while the hook is disabled.
        -- Leave request state unchanged for this phase.
        return nil
    end,
}
return hook
