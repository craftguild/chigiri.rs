//! Domain type with no dependency on Chigiri or the generated crate.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterKind {
    V3,
    V4,
}

pub struct Item {
    pub kind: AdapterKind,
}
