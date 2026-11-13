//! Converts resolver groups into object-safe adapters.

mod ordinary;
mod subscription;

pub(super) use ordinary::render_group;
