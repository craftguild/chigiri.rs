//! Describe fixed hook value types without parsing Luau type strings or prose.

#[derive(Clone, Copy)]
pub(crate) enum HookValue {
    Text,
    Number,
    Json,
    JsonObject,
    Null,
    Named(&'static str),
    Array(&'static Self),
    Union(&'static [Self]),
    Enumeration(&'static [&'static str]),
    Optional(&'static Self),
}

pub(crate) struct FunctionSpec {
    pub name: &'static str,
    pub alias: &'static str,
    pub arguments: &'static [(&'static str, HookValue)],
    pub diverges: bool,
}

impl FunctionSpec {
    pub fn minimum_arity(&self) -> usize {
        self.arguments
            .iter()
            .take_while(|(_, ty)| !matches!(ty, HookValue::Optional(_)))
            .count()
    }
    pub fn accepts_arity(&self, length: usize) -> bool {
        (self.minimum_arity()..=self.arguments.len()).contains(&length)
    }
}

pub(crate) const LOG_FUNCTION: FunctionSpec = FunctionSpec {
    name: super::hooks::LOG,
    alias: "ChigiriHookLog",
    arguments: &[
        ("level", HookValue::Enumeration(&super::hooks::LEVELS)),
        ("message", HookValue::Text),
        ("fields", HookValue::Optional(&HookValue::JsonObject)),
    ],
    diverges: false,
};
pub(crate) const REJECT_FUNCTION: FunctionSpec = FunctionSpec {
    name: super::hooks::REJECT,
    alias: "ChigiriHookReject",
    arguments: &[("reason", HookValue::Named("ChigiriHookRejectReason"))],
    diverges: true,
};
