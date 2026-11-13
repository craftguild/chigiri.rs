//! Stable directive purposes, examples, rules, and related reference entries.

use super::directive_types::DirectiveExampleDoc;

pub(super) struct Metadata {
    pub purpose: &'static str,
    pub rules: &'static [&'static str],
    pub related: &'static [&'static str],
    pub examples: &'static [Example],
}

pub(super) struct Example {
    pub id: &'static str,
    pub title: &'static str,
    pub valid: bool,
    pub sdl: &'static str,
    pub expected: &'static str,
}

impl Example {
    pub(super) fn document(&self) -> DirectiveExampleDoc {
        DirectiveExampleDoc {
            id: self.id.into(),
            title: self.title.into(),
            valid: self.valid,
            sdl: self.sdl.into(),
            expected: self.expected.into(),
        }
    }
}

pub(super) fn get(name: &str) -> Metadata {
    match name {
        "skip" => standard(
            "Exclude a selection when the condition is true.",
            &["directives include"],
        ),
        "include" => standard(
            "Include a selection only when the condition is true.",
            &["directives skip"],
        ),
        "deprecated" => standard(
            "Mark a field, argument, input field, or enum value as deprecated.",
            &[],
        ),
        "specifiedBy" => standard(
            "Associate a custom scalar with its external specification URL.",
            &["directives representation"],
        ),
        "oneOf" => standard("Require exactly one field in the input object.", &[]),
        "relation" => mock_directive(
            "Declare an object relation and its stored reference fields.",
            &["relation.validation", "relation.shape"],
            &["methods connect", "methods disconnect"],
            RELATION_EXAMPLES,
        ),
        "virtual" => mock_directive(
            "Declare a resolver-backed field without a stored slot.",
            &["scope.write"],
            &["methods find"],
            VIRTUAL_EXAMPLES,
        ),
        "constraint" => mock_directive(
            "Declares value constraints on fields, input fields, and arguments.",
            &[
                "constraint.locations",
                "constraint.formats",
                "constraint.regex_dialect",
            ],
            &["directives representation", "limits"],
            CONSTRAINT_EXAMPLES,
        ),
        "representation" => mock_directive(
            "Declare the value representation and base constraints for a custom scalar.",
            &["constraint.wrappers", "value.number"],
            &["directives constraint"],
            REPRESENTATION_EXAMPLES,
        ),
        _ => Metadata {
            purpose: "User-defined directives have no Mock execution semantics.",
            rules: &[],
            related: &[],
            examples: &[],
        },
    }
}

fn standard(purpose: &'static str, related: &'static [&'static str]) -> Metadata {
    Metadata {
        purpose,
        rules: &[],
        related,
        examples: &[],
    }
}

fn mock_directive(
    purpose: &'static str,
    rules: &'static [&'static str],
    related: &'static [&'static str],
    examples: &'static [Example],
) -> Metadata {
    Metadata {
        purpose,
        rules,
        related,
        examples,
    }
}

const RELATION_EXAMPLES: &[Example] = &[
    Example {
        id: "directive.relation.valid",
        title: "Match an owner field to the target key.",
        valid: true,
        sdl: "type User { id: ID! }\ntype Post { authorId: ID! author: User! @relation(fields: [\"authorId\"], references: [\"id\"]) }\ntype Query { post: Post }",
        expected: "Mock IR builds the owner relation.",
    },
    Example {
        id: "directive.relation.invalid",
        title: "Reject a relation that omits its required storage field.",
        valid: false,
        sdl: "type User { id: ID! }\ntype Post { author: User @relation(fields: [], references: [\"id\"]) }\ntype Query { post: Post }",
        expected: "invalid relation component names",
    },
];

const VIRTUAL_EXAMPLES: &[Example] = &[
    Example {
        id: "directive.virtual.valid",
        title: "Only the resolver returns the value field.",
        valid: true,
        sdl: "type Query { greeting: String @virtual }",
        expected: "Mock IR builds a virtual resolver.",
    },
    Example {
        id: "directive.virtual.invalid",
        title: "Reject @virtual on a type definition.",
        valid: false,
        sdl: "type Query @virtual { greeting: String }",
        expected: "invalid SDL",
    },
];

const CONSTRAINT_EXAMPLES: &[Example] = &[
    Example {
        id: "directive.constraint.valid",
        title: "Apply length and date-time constraints to a field.",
        valid: true,
        sdl: "type Query { createdAt: String @constraint(minLength: 1, format: \"date-time\") }",
        expected: "Mock IR builds two string constraints.",
    },
    Example {
        id: "directive.constraint.invalid",
        title: "Reject a string constraint on a numeric field.",
        valid: false,
        sdl: "type Query { count: Int @constraint(minLength: 1) }",
        expected: "invalid value constraints",
    },
];

const REPRESENTATION_EXAMPLES: &[Example] = &[
    Example {
        id: "directive.representation.valid",
        title: "Represent a custom scalar as STRING.",
        valid: true,
        sdl: "scalar DateTime @representation(type: STRING)\ntype Query { now: DateTime }",
        expected: "Mock IR builds a STRING representation.",
    },
    Example {
        id: "directive.representation.invalid",
        title: "Type is required.",
        valid: false,
        sdl: "scalar DateTime @representation\ntype Query { now: DateTime }",
        expected: "invalid SDL",
    },
];
