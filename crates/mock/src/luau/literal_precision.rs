//! Preserve literal precision at syntax-proven query and source option positions.

use std::collections::BTreeMap;

use full_moon::ast::{Expression, Field, TableConstructor};
use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use full_moon::visitors::Visitor;

pub(super) use super::literal_precision_sites::{ColumnMap, TypeSite};
use super::literal_precision_syntax;
use super::literal_precision_types::{self, ExpectedSite};
use super::literal_string::decoded as string_token;
use super::names;
use crate::ir::MockIr;

pub(super) fn narrow(source: &str, ir: &MockIr) -> (String, ColumnMap, Vec<TypeSite>) {
    let parsed = full_moon::parse_fallible(source, full_moon::LuaVersion::luau());
    if !parsed.errors().is_empty() {
        return (source.to_owned(), ColumnMap::default(), Vec::new());
    }
    let mut collector = Collector {
        ir,
        host_contexts: super::terminal_flow::host_context_names(ir, parsed.ast()),
        aliases: literal_precision_syntax::AliasScopes::default(),
        additions: BTreeMap::new(),
        block_bindings: BTreeMap::new(),
        return_sites: Vec::new(),
        sites: Vec::new(),
    };
    collector.visit_ast(parsed.ast());
    let mut narrowed = source.to_owned();
    let mut map = ColumnMap::new(source);
    for (&byte, suffix) in collector.additions.iter().rev() {
        narrowed.insert_str(byte, suffix);
        map.add(byte, suffix.len());
    }
    map.finish();
    (narrowed, map, collector.sites)
}

pub(super) struct Collector<'a> {
    pub(super) ir: &'a MockIr,
    pub(super) host_contexts: std::collections::BTreeSet<String>,
    pub(super) aliases: literal_precision_syntax::AliasScopes,
    pub(super) additions: BTreeMap<usize, String>,
    pub(super) block_bindings: BTreeMap<usize, Vec<String>>,
    pub(super) return_sites: Vec<Option<ExpectedSite>>,
    pub(super) sites: Vec<TypeSite>,
}

impl Collector<'_> {
    pub(super) fn options(
        &mut self,
        model: Option<&str>,
        method: &str,
        options_type: crate::api::TypeExpr,
        options: &TableConstructor,
    ) {
        if let Some(alias) = model.and_then(|model| self.options_alias(model, options_type)) {
            self.contextual_table(options, &alias);
        }
        for field in options.fields().iter() {
            let Some((name, value)) = named_field(field) else {
                continue;
            };
            if !query_option(options_type, &name) {
                if let Some(model) = model
                    && let Some(site) =
                        TypeSite::invalid_option(value, model, method, query_options(options_type))
                {
                    self.sites.push(site);
                }
                self.contextual_expression(value, "never");
                continue;
            }
            match (name.as_str(), value) {
                ("where", Expression::TableConstructor(predicate)) => {
                    self.predicate(model, predicate);
                }
                ("orderBy", Expression::TableConstructor(keys)) => self.order_keys(model, keys),
                ("limit" | "offset", value)
                    if options_type == crate::api::TypeExpr::FindOptions =>
                {
                    if let Some(model) = model {
                        let expected =
                            literal_precision_types::integer_option(model, method, &name);
                        self.push_literal(value, &expected, None);
                    }
                }
                _ => {}
            }
        }
    }

    fn order_keys(&mut self, model: Option<&str>, keys: &TableConstructor) {
        for field in keys.fields().iter() {
            let Field::NoKey(Expression::TableConstructor(key)) = field else {
                continue;
            };
            if let Some(alias) = model.and_then(|model| self.model_alias(model, "OrderKey")) {
                self.contextual_table(key, &alias);
            }
            for field in key.fields().iter() {
                let Some((name, value)) = named_field(field) else {
                    continue;
                };
                if name == "direction"
                    && let Some(model) = model
                {
                    let expected = literal_precision_types::order_direction(model);
                    let actual = string_token(value)
                        .is_some()
                        .then_some("string outside sort direction");
                    self.push_literal(value, &expected, actual);
                }
            }
        }
    }

    fn predicate(&mut self, model: Option<&str>, predicate: &TableConstructor) {
        if let Some(alias) = model.and_then(|model| self.model_alias(model, "Where")) {
            self.contextual_table(predicate, &alias);
        }
        for field in predicate.fields().iter() {
            let Some((name, value)) = named_field(field) else {
                continue;
            };
            if matches!(name.as_str(), "all" | "any") {
                self.predicate_group(model, value);
            }
        }
        let field = named_value(predicate, "field");
        let operator = named_value(predicate, "op");
        let value = named_value(predicate, "value");
        let (Some(field), Some(operator), Some(value)) = (field, operator, value) else {
            return;
        };
        let Some((_, field_name)) = string_token(field) else {
            return;
        };
        let Some((_, operator_name)) = string_token(operator) else {
            return;
        };
        let Some(context) = model.and_then(|model| {
            literal_precision_types::field(self.ir, model, &field_name, &operator_name)
        }) else {
            return;
        };
        if let Some(item) = context.item.as_ref() {
            self.push_literal(value, &context.value, None);
            if let Some(alias) = context.alias.as_deref() {
                self.enum_sites(value, alias, item);
            } else {
                self.list_item_sites(value, item);
            }
        } else if let Some(alias) = context.alias.as_deref() {
            if string_token(value).is_some() {
                self.enum_sites(value, alias, &context.value);
            } else {
                self.push_literal(value, &context.value, None);
            }
        } else {
            self.push_literal(value, &context.value, None);
        }
    }

    fn predicate_group(&mut self, model: Option<&str>, value: &Expression) {
        let Expression::TableConstructor(predicates) = value else {
            return;
        };
        for field in predicates.fields().iter() {
            if let Field::NoKey(Expression::TableConstructor(predicate)) = field {
                self.predicate(model, predicate);
            }
        }
    }

    fn enum_sites(&mut self, value: &Expression, alias: &str, expected: &ExpectedSite) {
        if string_token(value).is_some() {
            self.contextual_expression(value, alias);
            self.push_literal(value, expected, Some("string outside declared enum"));
            return;
        }
        let Expression::TableConstructor(values) = value else {
            return;
        };
        for field in values.fields().iter() {
            if let Field::NoKey(value) = field {
                let actual = string_token(value).map(|_| {
                    self.contextual_expression(value, alias);
                    "string outside declared enum"
                });
                self.push_literal(value, expected, actual);
            }
        }
    }

    fn list_item_sites(&mut self, value: &Expression, expected: &ExpectedSite) {
        let Expression::TableConstructor(values) = value else {
            return;
        };
        for field in values.fields().iter() {
            if let Field::NoKey(value) = field {
                self.push_literal(value, expected, None);
            }
        }
    }

    pub(super) fn push_literal(
        &mut self,
        value: &Expression,
        expected: &ExpectedSite,
        actual: Option<&'static str>,
    ) -> bool {
        let Some(site) = TypeSite::mismatch(value, expected, actual) else {
            return false;
        };
        self.sites.push(site);
        true
    }

    fn options_alias(&self, model: &str, options_type: crate::api::TypeExpr) -> Option<String> {
        let prefix = match options_type {
            crate::api::TypeExpr::FindOptions => "FindOptions",
            crate::api::TypeExpr::CountOptions => "CountOptions",
            crate::api::TypeExpr::TargetOptions => "FindOptions",
            _ => return None,
        };
        self.model_alias(model, prefix)
    }

    fn model_alias(&self, model: &str, prefix: &str) -> Option<String> {
        let ty = self.ir.graph().type_id(model)?;
        Some(names::ty(self.ir, ty, prefix))
    }

    fn contextual_table(&mut self, table: &TableConstructor, alias: &str) {
        let Some((start, end)) = table.range() else {
            return;
        };
        let binding = format!("_chigiri_checked_{}", start.bytes());
        self.additions.entry(start.bytes()).or_insert_with(|| {
            format!("(function({binding}: {alias}): {alias} return {binding} end)(")
        });
        self.additions
            .entry(end.bytes())
            .or_insert_with(|| ")".to_owned());
    }

    pub(super) fn contextual_expression(&mut self, expression: &Expression, annotation: &str) {
        let Some((start, end)) = expression.range() else {
            return;
        };
        let binding = format!("_chigiri_checked_{}", start.bytes());
        self.additions.entry(start.bytes()).or_insert_with(|| {
            format!("(function({binding}: {annotation}): {annotation} return {binding} end)(")
        });
        self.additions
            .entry(end.bytes())
            .or_insert_with(|| ")".to_owned());
    }

    pub(super) fn update_patch(&mut self, model: &str, patch: &TableConstructor) {
        let Some(ty) = self.ir.graph().type_id(model) else {
            return;
        };
        let Some(model) = self.ir.model_for(ty) else {
            return;
        };
        let immutable = &self.ir.models()[model.0].immutable_slots;
        for field in patch.fields().iter() {
            let Some((name, value)) = named_field(field) else {
                continue;
            };
            if immutable
                .iter()
                .any(|id| self.ir.slots()[id.0].name.as_ref() == name)
            {
                self.contextual_expression(value, "never");
            }
        }
    }
}

fn named_value<'a>(table: &'a TableConstructor, expected: &str) -> Option<&'a Expression> {
    table.fields().iter().find_map(|field| {
        let (name, value) = named_field(field)?;
        (name == expected).then_some(value)
    })
}

fn query_option(options_type: crate::api::TypeExpr, name: &str) -> bool {
    query_options(options_type)
        .iter()
        .any(|option| option.name == name)
}

fn query_options(options_type: crate::api::TypeExpr) -> &'static [crate::api::OptionSpec] {
    match options_type {
        crate::api::TypeExpr::CountOptions => crate::api::COUNT_OPTIONS,
        crate::api::TypeExpr::FindOptions | crate::api::TypeExpr::TargetOptions => {
            crate::api::FIND_OPTIONS
        }
        _ => &[],
    }
}

fn named_field(field: &Field) -> Option<(String, &Expression)> {
    match field {
        Field::NameKey { key, value, .. } => {
            let TokenType::Identifier { identifier } = key.token().token_type() else {
                return None;
            };
            Some((identifier.to_string(), value))
        }
        Field::ExpressionKey { key, value, .. } => {
            super::literal_string::decoded(key).map(|(_, name)| (name, value))
        }
        _ => None,
    }
}
