//! Rechecks parser syntax invariants on programmatically constructed public AST values before composition.

mod members;
mod values;

use crate::language::ast::{Definition, Document, Name, TypeSystemDefinition};
use crate::{ExecutionLimits, Span};

use super::composer::Composer;

struct Validator<'a> {
    composer: &'a mut Composer,
    max_type_layers: usize,
    max_value_nesting: usize,
}

pub(super) fn validate(composer: &mut Composer, documents: &[Document], limits: &ExecutionLimits) {
    let mut validator = Validator {
        composer,
        // Parser-derived TypeRef has a maximum of one NonNull in each list hierarchy.
        max_type_layers: limits
            .max_parser_nesting
            .saturating_mul(2)
            .saturating_add(1),
        max_value_nesting: limits.max_parser_nesting,
    };
    for document in documents {
        for definition in &document.definitions {
            if !validator.can_continue() {
                return;
            }
            if let Definition::TypeSystem(definition) = definition {
                validator.definition(definition);
            }
        }
    }
}

impl Validator<'_> {
    fn can_continue(&self) -> bool {
        self.composer.can_continue()
    }

    fn definition(&mut self, definition: &TypeSystemDefinition) {
        match definition {
            TypeSystemDefinition::Schema(definition) => {
                self.directives(&definition.directives);
                for root in &definition.root_operations {
                    self.name(&root.named_type, "root operation type");
                }
            }
            TypeSystemDefinition::Scalar(definition) => {
                self.name(&definition.name, "scalar type");
                self.directives(&definition.directives);
            }
            TypeSystemDefinition::Object(definition) => {
                self.name(&definition.name, "object type");
                self.names(&definition.implements_interfaces, "implemented interface");
                self.directives(&definition.directives);
                self.fields(&definition.fields);
            }
            TypeSystemDefinition::Interface(definition) => {
                self.name(&definition.name, "interface type");
                self.names(&definition.implements_interfaces, "implemented interface");
                self.directives(&definition.directives);
                self.fields(&definition.fields);
            }
            TypeSystemDefinition::Union(definition) => {
                self.name(&definition.name, "union type");
                self.names(&definition.members, "union member");
                self.directives(&definition.directives);
            }
            TypeSystemDefinition::Enum(definition) => {
                self.name(&definition.name, "enum type");
                self.directives(&definition.directives);
                self.enum_values(&definition.values);
            }
            TypeSystemDefinition::InputObject(definition) => {
                self.name(&definition.name, "input object type");
                self.directives(&definition.directives);
                self.input_values(&definition.fields);
            }
            TypeSystemDefinition::Directive(definition) => {
                self.name(&definition.name, "directive definition");
                self.input_values(&definition.arguments);
            }
            TypeSystemDefinition::SchemaExtension(extension) => {
                self.require_non_empty(
                    !extension.directives.is_empty() || !extension.root_operations.is_empty(),
                    extension.span,
                    "Schema extension must add a directive or root operation",
                );
                self.directives(&extension.directives);
                for root in &extension.root_operations {
                    self.name(&root.named_type, "root operation type");
                }
            }
            TypeSystemDefinition::ScalarExtension(extension) => {
                self.require_non_empty(
                    !extension.directives.is_empty(),
                    extension.span,
                    "Scalar extension must add a directive",
                );
                self.name(&extension.name, "scalar extension target");
                self.directives(&extension.directives);
            }
            TypeSystemDefinition::ObjectExtension(extension) => {
                self.require_non_empty(
                    !extension.implements_interfaces.is_empty()
                        || !extension.directives.is_empty()
                        || !extension.fields.is_empty(),
                    extension.span,
                    "Object extension must add an interface, directive, or field",
                );
                self.name(&extension.name, "object extension target");
                self.names(&extension.implements_interfaces, "implemented interface");
                self.directives(&extension.directives);
                self.fields(&extension.fields);
            }
            TypeSystemDefinition::InterfaceExtension(extension) => {
                self.require_non_empty(
                    !extension.implements_interfaces.is_empty()
                        || !extension.directives.is_empty()
                        || !extension.fields.is_empty(),
                    extension.span,
                    "Interface extension must add an interface, directive, or field",
                );
                self.name(&extension.name, "interface extension target");
                self.names(&extension.implements_interfaces, "implemented interface");
                self.directives(&extension.directives);
                self.fields(&extension.fields);
            }
            TypeSystemDefinition::UnionExtension(extension) => {
                self.require_non_empty(
                    !extension.directives.is_empty() || !extension.members.is_empty(),
                    extension.span,
                    "Union extension must add a directive or member",
                );
                self.name(&extension.name, "union extension target");
                self.names(&extension.members, "union member");
                self.directives(&extension.directives);
            }
            TypeSystemDefinition::EnumExtension(extension) => {
                self.require_non_empty(
                    !extension.directives.is_empty() || !extension.values.is_empty(),
                    extension.span,
                    "Enum extension must add a directive or value",
                );
                self.name(&extension.name, "enum extension target");
                self.directives(&extension.directives);
                self.enum_values(&extension.values);
            }
            TypeSystemDefinition::InputObjectExtension(extension) => {
                self.require_non_empty(
                    !extension.directives.is_empty() || !extension.fields.is_empty(),
                    extension.span,
                    "Input object extension must add a directive or field",
                );
                self.name(&extension.name, "input object extension target");
                self.directives(&extension.directives);
                self.input_values(&extension.fields);
            }
        }
    }

    fn names(&mut self, names: &[Name], element: &str) {
        for name in names {
            if !self.can_continue() {
                return;
            }
            self.name(name, element);
        }
    }

    fn name(&mut self, name: &Name, element: &str) {
        if !self.can_continue() || valid_name(&name.value) {
            return;
        }
        self.invalid(
            format!("Invalid GraphQL Name `{}` for {element}", name.value),
            name.span,
        );
    }

    fn require_non_empty(&mut self, condition: bool, span: Span, message: &'static str) {
        if !condition {
            self.invalid(message, span);
        }
    }

    fn invalid(&mut self, message: impl Into<String>, span: Span) {
        self.composer
            .report(super::SCHEMA_INVALID_AST_SYNTAX, message, span);
    }

    fn nesting(&mut self, element: &str, span: Span) {
        self.composer.report(
            super::SCHEMA_AST_NESTING_LIMIT,
            format!("{element} nesting exceeds the configured parser limit"),
            span,
        );
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
