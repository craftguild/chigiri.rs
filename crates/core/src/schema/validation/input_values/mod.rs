//! Validates schema constant values with the shared input-coercion rules.

mod catalog;
mod check;
mod jobs;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::language::ast::ValueNode;

use super::super::composer::Composer;

#[derive(Clone)]
enum ExpectedType {
    Named(Arc<str>),
    List(Box<Self>),
    NonNull(Box<Self>),
}

enum ScalarShape {
    Int,
    Float,
    String,
    Boolean,
    Id,
    Custom,
}

struct InputFieldShape {
    ty: ExpectedType,
}

struct InputObjectShape {
    fields: BTreeMap<String, InputFieldShape>,
    required: Vec<String>,
    one_of: bool,
}

enum NamedShape {
    Scalar(ScalarShape),
    Enum(BTreeSet<String>),
    InputObject(InputObjectShape),
}

struct Catalog {
    named: BTreeMap<String, NamedShape>,
}

struct Job {
    expected: ExpectedType,
    value: ValueNode,
}

pub(super) fn validate(composer: &mut Composer) {
    let catalog = Catalog::build(&composer.schema);
    let jobs = jobs::collect(&composer.schema);
    for job in &jobs {
        if !composer.can_continue() {
            return;
        }
        check::validate(composer, &catalog, job);
    }
}
