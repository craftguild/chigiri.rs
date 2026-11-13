//! Recheck parser invariants for members, directives, and type references.

use crate::language::ast::{
    Directive, EnumValueDefinition, FieldDefinition, InputValueDefinition, TypeRef, TypeRefKind,
};

use super::Validator;

impl Validator<'_> {
    pub(super) fn directives(&mut self, directives: &[Directive]) {
        for directive in directives {
            if !self.can_continue() {
                return;
            }
            self.name(&directive.name, "directive application");
            if !self.can_continue() {
                return;
            }
            for argument in &directive.arguments {
                self.name(&argument.name, "directive argument");
                if !self.can_continue() {
                    return;
                }
                self.value(&argument.value);
                if !self.can_continue() {
                    return;
                }
            }
        }
    }

    pub(super) fn fields(&mut self, fields: &[FieldDefinition]) {
        for field in fields {
            if !self.can_continue() {
                return;
            }
            self.name(&field.name, "field");
            if !self.can_continue() {
                return;
            }
            self.input_values(&field.arguments);
            if !self.can_continue() {
                return;
            }
            self.type_ref(&field.ty);
            if !self.can_continue() {
                return;
            }
            self.directives(&field.directives);
        }
    }

    pub(super) fn input_values(&mut self, values: &[InputValueDefinition]) {
        for value in values {
            if !self.can_continue() {
                return;
            }
            self.name(&value.name, "input value definition");
            if !self.can_continue() {
                return;
            }
            self.type_ref(&value.ty);
            if !self.can_continue() {
                return;
            }
            if let Some(default) = &value.default_value {
                self.value(default);
            }
            if !self.can_continue() {
                return;
            }
            self.directives(&value.directives);
        }
    }

    pub(super) fn enum_values(&mut self, values: &[EnumValueDefinition]) {
        for value in values {
            if !self.can_continue() {
                return;
            }
            self.name(&value.name, "enum value");
            if !self.can_continue() {
                return;
            }
            self.directives(&value.directives);
        }
    }

    fn type_ref(&mut self, reference: &TypeRef) {
        if !self.can_continue() {
            return;
        }
        let mut current = reference;
        let mut layers = 1_usize;
        loop {
            if layers > self.max_type_layers {
                self.nesting("Type reference", current.span);
                return;
            }
            match &current.kind {
                TypeRefKind::Named(name) => {
                    self.name(name, "type reference");
                    return;
                }
                TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => {
                    current = inner;
                    layers += 1;
                }
            }
        }
    }
}
