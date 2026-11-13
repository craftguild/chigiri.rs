//! Stateful Luau Mock runtime built from a validated schema-derived IR.

#![forbid(unsafe_code)]

extern crate self as chigiri_mock;

mod api;
pub mod bundle;
pub mod cancellation;
pub mod checked;
#[cfg(test)]
mod checked_tests;
pub mod cli;
pub mod clock;
mod codec;
mod config;
mod diagnostic;
pub mod events;
mod files;
pub mod hooks;
pub mod info;
pub mod ir;
pub mod limits;
pub mod logging;
pub mod luau;
pub mod output;
mod process_owner;
#[cfg(test)]
mod process_owner_tests;
pub mod project;
mod protected_call;
mod publication;
pub mod reference;
pub mod runtime;
mod sandbox;
pub mod schema;
mod server;
pub mod service;
#[doc(hidden)]
pub mod stdio_writer;
pub mod store;
pub mod validation_control;
pub mod value;
mod vm_gate;
