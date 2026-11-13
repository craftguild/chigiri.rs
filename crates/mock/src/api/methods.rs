//! Single registry for model method names, handlers, signatures, and authority.

use std::collections::BTreeSet;

use super::{ArgumentSpec, Effect, FailureClass, ModelEligibility, RuleSpec, TypeExpr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Method {
    Find,
    Create,
    Update,
    Delete,
    Count,
    Connect,
    Disconnect,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MethodSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub handler: Method,
    pub effect: Effect,
    pub eligibility: ModelEligibility,
    pub arguments: &'static [ArgumentSpec],
    pub returns: TypeExpr,
    pub rule: RuleSpec,
}

impl MethodSpec {
    pub fn arity(&self) -> usize {
        // The Luau colon-call receiver is part of the host arity.
        self.arguments.len() + 1
    }
}

pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec {
        id: "model.count",
        name: "count",
        handler: Method::Count,
        effect: Effect::Read,
        eligibility: ModelEligibility::All,
        arguments: &[ArgumentSpec {
            name: "options",
            ty: TypeExpr::CountOptions,
        }],
        returns: TypeExpr::Number,
        rule: RuleSpec {
            id: "count.exact",
            description: "Return the exact number of records matching the query without materializing or paginating record copies.",
            failure: FailureClass::Query,
            example: "count.records",
        },
    },
    MethodSpec {
        id: "model.find",
        name: "find",
        handler: Method::Find,
        effect: Effect::Read,
        eligibility: ModelEligibility::All,
        arguments: &[ArgumentSpec {
            name: "options",
            ty: TypeExpr::FindOptions,
        }],
        returns: TypeExpr::Records,
        rule: RuleSpec {
            id: "find.snapshot",
            description: "Query the scope-visible snapshot or mutation candidate and return detached record copies in deterministic order.",
            failure: FailureClass::Query,
            example: "find.records",
        },
    },
    MethodSpec {
        id: "model.create",
        name: "create",
        handler: Method::Create,
        effect: Effect::Write,
        eligibility: ModelEligibility::All,
        arguments: &[ArgumentSpec {
            name: "input",
            ty: TypeExpr::CreateInput,
        }],
        returns: TypeExpr::Record,
        rule: RuleSpec {
            id: "create.generated-id",
            description: "Generate a lowercase ULID only when an eligible ID field is omitted.",
            failure: FailureClass::Storage,
            example: "create.record",
        },
    },
    MethodSpec {
        id: "model.update",
        name: "update",
        handler: Method::Update,
        effect: Effect::Write,
        eligibility: ModelEligibility::All,
        arguments: &[
            ArgumentSpec {
                name: "options",
                ty: TypeExpr::FindOptions,
            },
            ArgumentSpec {
                name: "patch",
                ty: TypeExpr::UpdateInput,
            },
        ],
        returns: TypeExpr::Records,
        rule: RuleSpec {
            id: "update.outer-patch",
            description: "Replace selected top-level fields, distinguish omitted values from ctx.null, preserve keys and registration order, and return updated records in selection order.",
            failure: FailureClass::Storage,
            example: "update.patch",
        },
    },
    MethodSpec {
        id: "model.delete",
        name: "delete",
        handler: Method::Delete,
        effect: Effect::Write,
        eligibility: ModelEligibility::All,
        arguments: &[ArgumentSpec {
            name: "options",
            ty: TypeExpr::FindOptions,
        }],
        returns: TypeExpr::Number,
        rule: RuleSpec {
            id: "delete.restrict",
            description: "Delete selected records and return their count. External required references restrict deletion; internal and implicit edges are removed consistently.",
            failure: FailureClass::Relation,
            example: "delete.record",
        },
    },
    MethodSpec {
        id: "model.connect",
        name: "connect",
        handler: Method::Connect,
        effect: Effect::Write,
        eligibility: ModelEligibility::Relations,
        arguments: RELATION_ARGUMENTS,
        returns: TypeExpr::Records,
        rule: RuleSpec {
            id: "connect.owner",
            description: "Connect selected source and target records, update both declared sides consistently, and return source records in selection order.",
            failure: FailureClass::Relation,
            example: "connect.owner",
        },
    },
    MethodSpec {
        id: "model.disconnect",
        name: "disconnect",
        handler: Method::Disconnect,
        effect: Effect::Write,
        eligibility: ModelEligibility::Relations,
        arguments: RELATION_ARGUMENTS,
        returns: TypeExpr::Records,
        rule: RuleSpec {
            id: "disconnect.owner",
            description: "Disconnect only selected edges on both declared sides and return source records in selection order.",
            failure: FailureClass::Relation,
            example: "disconnect.owner",
        },
    },
];

const RELATION_ARGUMENTS: &[ArgumentSpec] = &[
    ArgumentSpec {
        name: "sourceOptions",
        ty: TypeExpr::FindOptions,
    },
    ArgumentSpec {
        name: "relation",
        ty: TypeExpr::Relation,
    },
    ArgumentSpec {
        name: "targetOptions",
        ty: TypeExpr::TargetOptions,
    },
];

/// Reject incomplete, duplicate, or internally inconsistent method registries.
pub(crate) fn validate(specs: &[MethodSpec]) -> Result<(), &'static str> {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    let mut rules = BTreeSet::new();
    for spec in specs {
        if spec.id.is_empty()
            || spec.name.is_empty()
            || !ids.insert(spec.id)
            || !names.insert(spec.name)
            || !handlers.insert(spec.handler)
            || !rules.insert(spec.rule.id)
            || spec.rule.id.is_empty()
            || spec.rule.description.is_empty()
            || spec.rule.example.is_empty()
            || spec.arguments.is_empty()
            || spec.arguments.iter().any(|arg| arg.name.is_empty())
        {
            return Err("invalid model API contract");
        }
    }
    if handlers
        != BTreeSet::from([
            Method::Find,
            Method::Create,
            Method::Update,
            Method::Delete,
            Method::Count,
            Method::Connect,
            Method::Disconnect,
        ])
    {
        return Err("incomplete model API contract");
    }
    Ok(())
}
