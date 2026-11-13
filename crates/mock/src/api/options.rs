//! Share closed query options, defaults, and upper bounds between host validation and declarations.

#[derive(Clone, Copy, Debug)]
pub(crate) struct IntegerOption {
    pub default: Option<usize>,
    pub maximum: usize,
}

pub(crate) const FIND_LIMIT: IntegerOption = IntegerOption {
    default: None,
    maximum: 9_007_199_254_740_991,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum OptionType {
    Where,
    OrderBy,
    Integer(IntegerOption),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OptionSpec {
    pub name: &'static str,
    pub ty: OptionType,
    pub nullable: bool,
}

pub(crate) const FIND_OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        name: "where",
        ty: OptionType::Where,
        nullable: true,
    },
    OptionSpec {
        name: "orderBy",
        ty: OptionType::OrderBy,
        nullable: false,
    },
    OptionSpec {
        name: "offset",
        ty: OptionType::Integer(FIND_OFFSET),
        nullable: false,
    },
    OptionSpec {
        name: "limit",
        ty: OptionType::Integer(FIND_LIMIT),
        nullable: false,
    },
];

pub(crate) const FIND_OFFSET: IntegerOption = IntegerOption {
    default: Some(0),
    maximum: 9_007_199_254_740_991,
};
pub(crate) const COUNT_OPTIONS: &[OptionSpec] = &[FIND_OPTIONS[0]];
