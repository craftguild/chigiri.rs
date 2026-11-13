//! Identity types shared across schema, document, operation, and runtime boundaries.

use std::fmt;

const DIGEST_BYTES: usize = 32;
const OPERATION_NAME_BYTES: usize = 255;

macro_rules! digest_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; DIGEST_BYTES]);

        impl $name {
            /// Creates a digest from 32 SHA-256 bytes.
            pub const fn from_bytes(bytes: [u8; DIGEST_BYTES]) -> Self {
                Self(bytes)
            }

            /// Parse a digest from lowercase hexadecimal.
            pub fn from_hex(value: &str) -> Result<Self, DigestParseError> {
                parse_digest(value).map(Self)
            }

            /// Returns the SHA-256 bytes.
            pub const fn as_bytes(&self) -> &[u8; DIGEST_BYTES] {
                &self.0
            }

            /// Encodes the digest as lowercase hexadecimal.
            pub fn to_hex(self) -> String {
                encode_digest(self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.to_hex())
                    .finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }
    };
}

digest_type!(
    SchemaDigest,
    "The canonical SHA-256 of the generated schema source."
);
digest_type!(
    DocumentDigest,
    "SHA-256 digest of the exact GraphQL document bytes."
);
digest_type!(
    ResultShapeDigest,
    "The result shape of the selection operation is SHA-256."
);

/// Parse error for a digest that is not 64 lowercase hexadecimal characters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestParseError;

impl fmt::Display for DigestParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("digest must be 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for DigestParseError {}

/// GraphQL Name is the name of an operation that meets the grammar and byte limits.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct OperationName(String);

impl OperationName {
    /// Validates and owns an operation name.
    pub fn new(value: impl Into<String>) -> Result<Self, OperationNameError> {
        let value = value.into();
        if value.len() > OPERATION_NAME_BYTES || !is_graphql_name(&value) {
            return Err(OperationNameError);
        }
        Ok(Self(value))
    }

    /// Returns the validated operation name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OperationName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("OperationName")
            .field(&self.0)
            .finish()
    }
}

/// The operation name does not meet the GraphQL Name or byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationNameError;

impl fmt::Display for OperationNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation name must be a GraphQL Name within 255 bytes")
    }
}

impl std::error::Error for OperationNameError {}

/// Server-side key that binds an exact document to a selected operation.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct OperationKey {
    document_digest: DocumentDigest,
    operation_name: Option<OperationName>,
}

impl OperationKey {
    /// Creates a key from a document digest and a named or anonymous operation.
    pub const fn new(
        document_digest: DocumentDigest,
        operation_name: Option<OperationName>,
    ) -> Self {
        Self {
            document_digest,
            operation_name,
        }
    }

    /// Returns the exact document digest.
    pub const fn document_digest(&self) -> DocumentDigest {
        self.document_digest
    }

    /// Return the verified operation name, when the operation is named.
    pub const fn operation_name(&self) -> Option<&OperationName> {
        self.operation_name.as_ref()
    }
}

impl fmt::Debug for OperationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperationKey")
            .field("document_digest", &self.document_digest)
            .field("named", &self.operation_name.is_some())
            .finish()
    }
}

/// The immutable identity of the selected operation on a schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationIdentity {
    schema_digest: SchemaDigest,
    key: OperationKey,
}

impl OperationIdentity {
    /// Combines a schema digest with an operation key.
    pub const fn new(schema_digest: SchemaDigest, key: OperationKey) -> Self {
        Self { schema_digest, key }
    }

    /// Returns the schema digest.
    pub const fn schema_digest(&self) -> SchemaDigest {
        self.schema_digest
    }

    /// Returns the document-and-operation key.
    pub const fn key(&self) -> &OperationKey {
        &self.key
    }
}

fn parse_digest(value: &str) -> Result<[u8; DIGEST_BYTES], DigestParseError> {
    if value.len() != DIGEST_BYTES * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(DigestParseError);
    }
    let mut output = [0; DIGEST_BYTES];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_value(pair[0]) << 4) | hex_value(pair[1]);
    }
    Ok(output)
}

fn encode_digest(value: [u8; DIGEST_BYTES]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(DIGEST_BYTES * 2);
    for byte in value {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

const fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

fn is_graphql_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    matches!(first, b'A'..=b'Z' | b'a'..=b'z' | b'_')
        && bytes.all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::{DocumentDigest, OperationKey, OperationName, SchemaDigest};

    #[test]
    fn digest_is_only_a_lowercase_hex() {
        let text = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(SchemaDigest::from_hex(text).unwrap().to_hex(), text);
        assert!(DocumentDigest::from_hex(&text.to_uppercase()).is_err());
    }

    #[test]
    fn operation_name_and_anonymous_are_distinguished_by_key() {
        let digest = DocumentDigest::from_bytes([7; 32]);
        let named = OperationKey::new(digest, Some(OperationName::new("Books").unwrap()));
        let anonymous = OperationKey::new(digest, None);
        assert_ne!(named, anonymous);
        assert!(OperationName::new("not-valid-name").is_err());
    }
}
