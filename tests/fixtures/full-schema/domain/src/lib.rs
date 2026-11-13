#![forbid(unsafe_code)]
//! Independent domain enums for the synthetic compatibility fixture.

/// An independent domain model corresponding to `FormatKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatKind {
    Value176,
    Value180,
    Value179,
}

/// An independent domain model corresponding to `ExternalTransport`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalTransport {
    Value168,
    Value169,
}

/// An independent domain model corresponding to `UnitKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitKind {
    Value173,
    Value075,
    Value174,
}
