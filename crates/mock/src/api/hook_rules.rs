//! Hook rules and examples shared by the runtime contract and text/JSON
//! reference output.

use super::FailureClass as F;
use crate::hooks::HookSlot;

pub(crate) const RULES: &[(&str, &str, HookSlot, F)] = &[
    (
        "hook.phase",
        "Rust owns the before → one call to next → after sequence. Models are read-only in every hook. All after hooks are observational; their failure does not replace a successful result or undo an earlier commit.",
        HookSlot::RequestAfter,
        F::Capability,
    ),
    (
        "hook.state",
        "state is a closed typed object initialized for each request. Only request.before and operation.before may return statePatch. A patch replaces listed fields atomically; it is not a nested merge, and omitted fields remain unchanged. ctx.null represents an explicit null.",
        HookSlot::OperationBefore,
        F::Capability,
    ),
    (
        "hook.reject",
        "The first rejection from each before hook is sticky. Status 200 is rejected, and invalid or unassigned statuses become 500. For multiple field rejections, the highest status is used. Statuses 204, 205, and 304 have no body. A rejection does not downgrade an independent transport failure.",
        HookSlot::FieldBefore,
        F::Capability,
    ),
    (
        "hook.snapshot",
        "field.before and its resolver use the same store snapshot. field.after observes the resolver outcome before child selection completes. Mutations commit or roll back using the existing root-field transaction units.",
        HookSlot::FieldAfter,
        F::Capability,
    ),
    (
        "hook.request_view",
        "The before-hook request preserves the raw body, URL with query, and duplicate headers, while host and port come from the actual listener binding. Host and Forwarded headers are not authoritative. Header values preserve UTF-8, double backslashes, and encode every non-UTF-8 byte as uppercase \\xHH.",
        HookSlot::RequestBefore,
        F::Capability,
    ),
    (
        "hook.log",
        "ctx.log is available in every hook slot. JSONL on stdout uses chigiri-mock-log/v5; startup notices use stderr. timestamp is an integer UTC Unix epoch in milliseconds. Local diagnostics use safe structural details and nullable verified source positions. Automatic logs omit bodies, queries, headers, tokens, state, and results. Do not place secrets, Authorization/Cookie values, or complete bodies in explicit messages or fields.",
        HookSlot::OperationAfter,
        F::Output,
    ),
    (
        "hook.log_loss",
        "Hook, access, and subscription records have separate reserved stdout queues; diagnostics use the independent stderr notice queue. Logging cannot block responses or readiness. Serialization overflow, a full queue, and output failures increment the original hook/access/subscription/notice loss class. Unconfirmed delivery is not success, and notifications are best effort. Shutdown within the shared five-second limit does not guarantee a complete flush or final record.",
        HookSlot::OperationAfter,
        F::Resource,
    ),
    (
        "hook.access",
        "Access logs record the final route status, the path without its query, and duration_ms from one monotonic clock. Cancellation before a response uses status=null. completed means server-side body processing ended; it does not guarantee client receipt. The mock request_id is not presented as Chigiri RequestIdentity.",
        HookSlot::RequestAfter,
        F::Output,
    ),
];

pub(crate) fn example(slot: HookSlot) -> &'static str {
    match slot {
        HookSlot::RequestBefore => {
            r#"local authorized = false
for _, header in ctx.request.headers do
    if header.name == "authorization" and header.value == "Bearer local-demo" then authorized = true end
end
if not authorized then ctx.reject({code = 401, message = "Authorization required"}) end
return nil"#
        }
        HookSlot::OperationBefore => {
            "-- Omitted state fields remain unchanged; patches must match the configured declaration.\nreturn {statePatch = {}}"
        }
        HookSlot::FieldBefore => {
            "if ctx.field.path[1] == \"denied\" then ctx.reject({code = 403, message = \"Field denied\"}) end\nreturn nil"
        }
        HookSlot::RequestAfter | HookSlot::OperationAfter | HookSlot::FieldAfter => {
            "ctx.log(\"info\", \"phase completed\", {error_count = #ctx.result.errors})\nreturn nil"
        }
    }
}
