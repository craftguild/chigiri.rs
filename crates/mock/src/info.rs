//! Keep portable TOML, host API, and reference versions distinct from binary build information.

/// Only accepted portable configuration format.
pub const CONFIG_FORMAT: &str = "chigiri-mock/v11";
/// Model API contract that is compatible with the configuration format.
pub const API_VERSION: &str = "chigiri-mock-api/v10";
/// Executable checker/runtime capability contract.
pub const CAPABILITY_VERSION: &str = crate::api::runtime::VERSION;
/// The format of the structured reference document.
pub const REFERENCE_FORMAT: &str = "chigiri-mock-reference/v13";
/// Binary version from package metadata, also reported in reference output.
pub const BINARY_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Exactly pinned VM and type-checker version.
pub const LUAUR_VERSION: &str = "0.1.8";
