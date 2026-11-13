//! Resolve format names through a closed vocabulary rather than extensible callbacks.

/// Closed set of eight formats defined by ADR-0038, without normalization or external lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Standard Base64 text.
    Byte,
    /// RFC 3339 date and time with a time-zone offset.
    DateTime,
    /// Gregorian calendar date.
    Date,
    /// One addr-spec.
    Email,
    /// IPv4 address.
    Ipv4,
    /// IPv6 address.
    Ipv6,
    /// ASCII URI with a scheme.
    Uri,
    /// Hyphenated hexadecimal UUID.
    Uuid,
}

impl Format {
    /// Complete format set shared by SDL validation and reference generation.
    pub const ALL: [Self; 8] = [
        Self::Byte,
        Self::DateTime,
        Self::Date,
        Self::Email,
        Self::Ipv4,
        Self::Ipv6,
        Self::Uri,
        Self::Uuid,
    ];

    /// Stable name shared by SDL parsing and generated reference output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Byte => "byte",
            Self::DateTime => "date-time",
            Self::Date => "date",
            Self::Email => "email",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Uri => "uri",
            Self::Uuid => "uuid",
        }
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.as_str() == name)
    }
}
