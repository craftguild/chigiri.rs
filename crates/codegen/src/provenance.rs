//! Build a digest of generated-source inputs.

use sha2::{Digest as _, Sha256};

use chigiri_core::SchemaDigest;

use crate::discovery::DiscoveredSource;

/// Computes the ADR-defined SHA-256 digest over discovered paths and contents.
pub(crate) fn schema_content_digest(sources: &[DiscoveredSource]) -> SchemaDigest {
    let mut digest = Sha256::new();
    for source in sources {
        update_bytes(&mut digest, source.relative_path.as_bytes());
        update_bytes(&mut digest, source.contents.as_bytes());
    }
    SchemaDigest::from_bytes(digest.finalize().into())
}

fn update_bytes(digest: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).expect("The schema source length is up to u64.");
    digest.update(length.to_be_bytes());
    digest.update(bytes);
}

pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::schema_content_digest;
    use crate::discovery::DiscoveredSource;

    fn source(path: &str, contents: &str) -> DiscoveredSource {
        DiscoveredSource {
            relative_path: crate::filesystem::PortableRelativePath::new(path).unwrap(),
            contents: contents.to_owned(),
        }
    }

    #[test]
    fn schema_digest_includes_source_order_paths_and_content_boundaries() {
        let sources = [
            source("a.graphql", "type Query { value: Int }\n"),
            source("nested/b.gql", "scalar Date\n"),
        ];

        assert_eq!(
            schema_content_digest(&sources).to_hex(),
            "cccb9d5cb8a2598906514a939c9fe268a76502e3dab4e7bd35262c5bef9d121b"
        );
        assert_ne!(
            schema_content_digest(&sources),
            schema_content_digest(&[sources[1].clone(), sources[0].clone()])
        );
    }
}
