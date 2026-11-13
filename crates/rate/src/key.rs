//! Bounded quota keys that do not expose raw subject or secret tokens.

use std::fmt;

const COMPONENT_BYTES: usize = 128;
const TOTAL_BYTES: usize = 384;

/// A quota subject represented as either missing or bounded opaque bytes.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum QuotaSubject {
    Missing,
    Value(Vec<u8>),
}

impl QuotaSubject {
    /// Validates and stores opaque subject bytes.
    pub fn value(value: impl Into<Vec<u8>>) -> Result<Self, QuotaKeyError> {
        let value = value.into();
        if value.is_empty() || value.len() > COMPONENT_BYTES {
            return Err(QuotaKeyError);
        }
        Ok(Self::Value(value))
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Self::Missing => &[],
            Self::Value(value) => value,
        }
    }
}

impl fmt::Debug for QuotaSubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("QuotaSubject::Missing"),
            Self::Value(value) => formatter
                .debug_struct("QuotaSubject::Value")
                .field("bytes", &value.len())
                .finish_non_exhaustive(),
        }
    }
}

/// A bounded provider key containing a namespace, subject, and optional operation class.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QuotaKey {
    namespace: Vec<u8>,
    subject: QuotaSubject,
    operation_class: Option<Vec<u8>>,
}

impl QuotaKey {
    /// Validates every component and the total byte limit.
    pub fn new(
        namespace: impl Into<Vec<u8>>,
        subject: QuotaSubject,
        operation_class: Option<Vec<u8>>,
    ) -> Result<Self, QuotaKeyError> {
        let namespace = namespace.into();
        if namespace.is_empty()
            || namespace.len() > COMPONENT_BYTES
            || operation_class
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > COMPONENT_BYTES)
        {
            return Err(QuotaKeyError);
        }
        let total = namespace
            .len()
            .checked_add(subject.bytes().len())
            .and_then(|total| total.checked_add(operation_class.as_ref().map_or(0, Vec::len)))
            .ok_or(QuotaKeyError)?;
        if total > TOTAL_BYTES {
            return Err(QuotaKeyError);
        }
        Ok(Self {
            namespace,
            subject,
            operation_class,
        })
    }

    pub fn namespace(&self) -> &[u8] {
        &self.namespace
    }
    pub const fn subject(&self) -> &QuotaSubject {
        &self.subject
    }
    pub fn operation_class(&self) -> Option<&[u8]> {
        self.operation_class.as_deref()
    }
}

impl fmt::Debug for QuotaKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuotaKey")
            .field("namespace_bytes", &self.namespace.len())
            .field("subject", &self.subject)
            .field(
                "operation_class_bytes",
                &self.operation_class.as_ref().map(Vec::len),
            )
            .finish()
    }
}

/// A quota key exceeded a component or total byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaKeyError;

impl fmt::Display for QuotaKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("quota key exceeds its bounded component contract")
    }
}

impl std::error::Error for QuotaKeyError {}

#[cfg(test)]
mod tests {
    use super::{QuotaKey, QuotaSubject};

    #[test]
    fn missing_subject_differs_from_an_empty_subject() {
        let missing = QuotaKey::new(b"stage-a".to_vec(), QuotaSubject::Missing, None).unwrap();
        assert!(QuotaSubject::value(Vec::new()).is_err());
        let key = QuotaKey::new(
            b"stage-b".to_vec(),
            QuotaSubject::value(b"secret".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        assert_ne!(missing, key);
        assert!(!format!("{key:?}").contains("secret"));
    }
}
