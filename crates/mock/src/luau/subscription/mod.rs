//! Generate the closed subscription-specific Luau declarations and module shape.

pub(super) mod declarations;
mod module;

pub use module::render_subscription_module;
