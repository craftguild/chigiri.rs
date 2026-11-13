#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Embedded GraphiQL assets served through the facade endpoint.

mod embedded;

/// Returns an embedded asset without performing file or network I/O.
#[doc(hidden)]
pub fn __asset(name: &str) -> Option<(&'static str, &'static [u8])> {
    embedded::ASSETS
        .iter()
        .find(|asset| asset.0 == name)
        .map(|asset| (asset.1, asset.2))
}
