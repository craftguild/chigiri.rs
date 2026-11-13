//! Bidirectional conversions between equivalent wire and external-domain bindings.

macro_rules! identity_enum {
    ($wire:ident, $from:ident, $to:ident) => {
        type $wire = contract::$wire;

        fn $from(value: contract::$wire) -> Result<Self::$wire, chigiri::EnumBindingError> {
            Ok(value)
        }

        fn $to(value: &Self::$wire) -> Result<contract::$wire, chigiri::EnumBindingError> {
            Ok(*value)
        }
    };
}

macro_rules! external_enum {
    ($wire:ident, $domain:ident, $from:ident, $to:ident, [$($variant:ident),+]) => {
        type $wire = domain::$domain;

        fn $from(value: contract::$wire) -> Result<Self::$wire, chigiri::EnumBindingError> {
            match value {
                $(contract::$wire::$variant => Ok(domain::$domain::$variant)),+
            }
        }

        fn $to(value: &Self::$wire) -> Result<contract::$wire, chigiri::EnumBindingError> {
            match value {
                $(domain::$domain::$variant => Ok(contract::$wire::$variant)),+
            }
        }
    };
}

pub(crate) use external_enum;
pub(crate) use identity_enum;
