use std::borrow::Borrow;
use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

/// Newtype that holds `ID` as a canonical string.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(String);

impl Id {
    /// Creates an ID from a string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the ID and returns its string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<String> for Id {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Id {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

macro_rules! integer_id {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl From<$integer> for Id {
                fn from(value: $integer) -> Self {
                    Self(value.to_string())
                }
            }
        )+
    };
}

integer_id!(i32, i64, u32, u64);

impl FromStr for Id {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(value))
    }
}
