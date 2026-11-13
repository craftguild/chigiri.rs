//! Axum adoption example with synthetic lifecycle and differential validation.

mod application;
mod domain;
pub mod graphql;
mod hooks;
mod resolvers;
mod server;

pub use chigiri_testkit::differential;
pub use server::{Endpoint, endpoint, router};
