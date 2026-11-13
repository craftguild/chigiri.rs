//! Platform-independent logical paths for generated and Mock-owned content.

use crate::HostError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_PATH_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 255;
const MAX_COMPONENTS: usize = 64;

/// A validated product-owned relative path with `/` separators on every host.
///
/// This type represents a logical inventory or project identity. It is not a
/// native user path and cannot be converted into ambient filesystem authority.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortableRelativePath(Box<str>);

impl PortableRelativePath {
    /// Validates one complete logical path.
    pub fn new(value: &str) -> Result<Self, HostError> {
        validate(value)?;
        Ok(Self(value.into()))
    }

    /// Returns the normalized `/`-separated identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the logical path components in order.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.split('/')
    }

    /// Returns the final logical component.
    pub fn file_name(&self) -> &str {
        self.components()
            .next_back()
            .expect("a validated portable path is nonempty")
    }

    /// Returns the logical parent, or `None` for a one-component path.
    pub fn parent(&self) -> Option<Self> {
        self.0
            .rfind('/')
            .map(|separator| Self(self.0[..separator].into()))
    }

    /// Appends another validated logical path.
    pub fn join(&self, suffix: &Self) -> Result<Self, HostError> {
        let length = self
            .0
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(suffix.0.len()))
            .ok_or(HostError::Limit)?;
        if length > MAX_PATH_BYTES
            || self.components().count() + suffix.components().count() > MAX_COMPONENTS
        {
            return Err(HostError::Limit);
        }
        Ok(Self(format!("{}/{}", self.0, suffix.0).into()))
    }
}

impl AsRef<str> for PortableRelativePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for PortableRelativePath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for PortableRelativePath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl std::fmt::Display for PortableRelativePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for PortableRelativePath {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for PortableRelativePath {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl TryFrom<&str> for PortableRelativePath {
    type Error = HostError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for PortableRelativePath {
    type Error = HostError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate(&value)?;
        Ok(Self(value.into_boxed_str()))
    }
}

impl Serialize for PortableRelativePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PortableRelativePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

fn validate(value: &str) -> Result<(), HostError> {
    if value.len() > MAX_PATH_BYTES {
        return Err(HostError::Limit);
    }
    if value.is_empty()
        || value.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || value.chars().any(char::is_control)
    {
        return Err(HostError::Path);
    }
    let mut count = 0usize;
    for component in value.split('/') {
        count = count.checked_add(1).ok_or(HostError::Limit)?;
        if count > MAX_COMPONENTS || component.len() > MAX_COMPONENT_BYTES {
            return Err(HostError::Limit);
        }
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.ends_with([' ', '.'])
            || reserved_device(component)
        {
            return Err(HostError::Path);
        }
    }
    Ok(())
}

fn reserved_device(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered_device(&upper, "COM")
        || numbered_device(&upper, "LPT")
}

fn numbered_device(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_COMPONENTS, MAX_PATH_BYTES, PortableRelativePath};
    use crate::HostError;

    #[test]
    fn host_02_accepts_and_preserves_portable_logical_identity() {
        let path = PortableRelativePath::new("resolvers/query/find-user.luau").unwrap();
        assert_eq!(path.as_str(), "resolvers/query/find-user.luau");
        assert_eq!(path.file_name(), "find-user.luau");
        assert_eq!(path.parent().unwrap().as_str(), "resolvers/query");
        assert_eq!(
            PortableRelativePath::new("generated")
                .unwrap()
                .join(&PortableRelativePath::new("types/user.rs").unwrap())
                .unwrap()
                .as_str(),
            "generated/types/user.rs"
        );
    }

    #[test]
    fn host_02_rejects_absolute_escaping_aliasing_and_ambiguous_paths() {
        for invalid in [
            "",
            "/root",
            "root/",
            "root//file",
            ".",
            "..",
            "root/../file",
            "C:/file",
            "server:share",
            r"root\file",
            "less<than",
            "greater>than",
            "quote\"name",
            "pipe|name",
            "question?.luau",
            "star*.luau",
            "root/\0file",
            "name.",
            "name ",
            "CON",
            "con.txt",
            "dir/PRN",
            "aux.json",
            "NUL",
            "COM1",
            "com9.log",
            "LPT1",
            "lpt9.txt",
            "CONIN$",
            "conout$.txt",
        ] {
            assert_eq!(
                PortableRelativePath::new(invalid),
                Err(HostError::Path),
                "unexpected admission for {invalid:?}"
            );
        }
    }

    #[test]
    fn host_02_distinguishes_capacity_from_invalid_identity() {
        let too_deep = std::iter::repeat_n("a", MAX_COMPONENTS + 1)
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(PortableRelativePath::new(&too_deep), Err(HostError::Limit));
        assert_eq!(
            PortableRelativePath::new(&"a".repeat(MAX_PATH_BYTES + 1)),
            Err(HostError::Limit)
        );
        assert_eq!(
            PortableRelativePath::new(&"a".repeat(256)),
            Err(HostError::Limit)
        );
    }
}
