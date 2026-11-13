//! Resolve manifest domain-enum bindings for semantic fingerprints and provenance.

use std::collections::BTreeSet;

use chigiri_core::executor::ResolverSchemaFingerprint;
use chigiri_core::schema::SemanticSchema;
use sha2::{Digest as _, Sha256};

use crate::error::CodegenError;
use crate::provenance::lowercase_hex;

#[derive(Clone, Debug)]
pub(crate) struct ConfigProvenance {
    label: String,
    digest_hex: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedBindings {
    names: BTreeSet<String>,
    provenance: Option<ConfigProvenance>,
}

impl ResolvedBindings {
    pub(crate) fn from_manifest(
        manifest: &crate::ResolvedManifest,
        schema: &SemanticSchema,
    ) -> Result<Self, CodegenError> {
        let names = manifest.manifest().resolve_domain_enums(schema)?;
        let digest: [u8; 32] = Sha256::digest(manifest.manifest_bytes()).into();
        Ok(Self {
            names,
            provenance: Some(ConfigProvenance {
                label: manifest.manifest_label().to_owned(),
                digest_hex: lowercase_hex(&digest),
            }),
        })
    }

    pub(crate) fn semantic_digest(&self) -> [u8; 32] {
        domain_enums_digest(&self.names)
    }

    pub(crate) fn names(&self) -> &BTreeSet<String> {
        &self.names
    }

    pub(crate) fn provenance(&self) -> Option<&ConfigProvenance> {
        self.provenance.as_ref()
    }

    pub(crate) fn resolver_abi_fingerprint(
        &self,
        schema: ResolverSchemaFingerprint,
    ) -> ResolverSchemaFingerprint {
        if self.names.is_empty() {
            return schema;
        }
        let mut hash = schema.hash();
        for byte in self.semantic_digest() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        ResolverSchemaFingerprint::new(schema.component_count().saturating_add(1), hash)
    }
}

pub(crate) fn domain_enums_digest(names: &BTreeSet<String>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"chigiri/domain-enums-v1\0");
    for name in names {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
    }
    digest.finalize().into()
}

impl ConfigProvenance {
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn digest_hex(&self) -> &str {
        &self.digest_hex
    }
}
