//! Bidirectional conversions for each enum bound through `SchemaTypes`.

macro_rules! bind_enum {
    ($wire:ident, $from:ident, $to:ident) => {
        type $wire = crate::types::DomainKind;

        fn $from(value: crate::generated::$wire) -> Result<Self::$wire, chigiri::EnumBindingError> {
            match value {
                crate::generated::$wire::Alpha => Ok(crate::types::DomainKind::Alpha),
                crate::generated::$wire::Beta => Ok(crate::types::DomainKind::Beta),
            }
        }

        fn $to(value: &Self::$wire) -> Result<crate::generated::$wire, chigiri::EnumBindingError> {
            Ok(match value {
                crate::types::DomainKind::Alpha => crate::generated::$wire::Alpha,
                crate::types::DomainKind::Beta => crate::generated::$wire::Beta,
            })
        }
    };
}

pub(crate) use bind_enum;
