//! Example connecting an SDL-defined schema to an HTTP endpoint.

mod domain;
pub mod graphql;
mod plugin;
mod resolvers;
mod server;

#[cfg(feature = "production-validation")]
#[doc(hidden)]
pub mod benchmark;

pub use domain::{Book, Catalog};
pub use server::{AppContext, endpoint};
