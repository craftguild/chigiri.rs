//! Fixed limits for Mock inputs, IR construction, execution, and owned output.

pub mod request;
pub mod warning;

use std::time::Duration;

/// Maximum byte length of configuration TOML.
pub const CONFIG_BYTES: usize = 256 * 1024;
/// Maximum byte length of inline seed JSON.
pub const SEED_BYTES: usize = 256 * 1024;
/// Maximum UTF-8 byte length of a model ID or relation key component.
pub const ID_BYTES: usize = 256;
/// Maximum number of user SDL sources.
pub const SDL_SOURCES: usize = 64;
/// Maximum number of schema types, excluding the embedded vocabulary.
pub const SDL_TYPES: usize = 256;
/// Maximum combined number of fields, arguments, and input-object members.
pub const SDL_MEMBERS: usize = 4096;
/// Maximum number of models.
pub const MODELS: usize = 64;
/// Maximum number of script coordinates.
pub const SCRIPTS: usize = 256;
/// Wait job queue capacity.
pub const QUEUED_JOBS: usize = 16;
/// Maximum accepted GraphQL HTTP requests, including one waiting at the worker boundary.
pub const HTTP_REQUESTS: usize = QUEUED_JOBS + 1;
/// Maximum byte length of raw HTTP text exposed to hooks, also charged to the shared value budget.
pub const HOOK_OBSERVATION_TEXT_BYTES: usize = 1024 * 1024;
/// The deadline for all operations from HTTP receipt.
pub const OPERATION_TIME: Duration =
    Duration::from_millis(request::policy(request::LimitKind::ElapsedMs).hard as u64);
/// The deadline for worker and output-writer recovery after a shutdown signal.
pub const SHUTDOWN_TIME: Duration = Duration::from_secs(5);
