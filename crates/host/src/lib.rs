//! Portable host capabilities shared by Chigiri's developer tools.
//!
//! This crate is infrastructure for Chigiri's own codegen and Mock workflows.
//! It is not re-exported by the application-facing framework facade.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
pub mod filesystem;
pub mod interrupt;
mod path;
pub mod process;
pub mod stdio;

pub use error::HostError;
pub use path::PortableRelativePath;
