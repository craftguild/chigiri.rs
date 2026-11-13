//! Shared digest calculation for exact document bytes used by catalogs and resolvers.

use chigiri_runtime::DocumentDigest;
use sha2::{Digest as _, Sha256};

pub(crate) fn document_digest(document: &str) -> DocumentDigest {
    DocumentDigest::from_bytes(Sha256::digest(document.as_bytes()).into())
}
