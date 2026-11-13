//! Resolves model receivers at their lexical scope and source position.

use std::collections::{BTreeMap, BTreeSet};

use full_moon::ast::{
    Expression, FunctionArgs, FunctionCall, Index, Prefix, Suffix, Var, VarExpression,
};
use full_moon::tokenizer::{TokenReference, TokenType};

enum Binding {
    Model(String),
    HostContext,
    Shadow,
}

#[derive(Default)]
pub(super) struct AliasScopes {
    scopes: Vec<BTreeMap<String, Binding>>,
    function_floors: Vec<usize>,
    captured_assignments: Vec<BTreeSet<String>>,
}

impl AliasScopes {
    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn enter_function(&mut self) {
        self.enter_scope();
        self.function_floors.push(self.scopes.len() - 1);
        self.captured_assignments.push(BTreeSet::new());
    }

    pub(super) fn exit_function(&mut self) {
        let captured = self.captured_assignments.pop().unwrap_or_default();
        self.function_floors.pop();
        self.exit_scope();
        for name in captured {
            self.assign(&name, None);
        }
    }

    pub(super) fn declare(&mut self, name: &str, model: Option<String>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), binding(model));
        }
    }

    pub(super) fn declare_host_context(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Binding::HostContext);
        }
    }

    pub(super) fn assign(&mut self, name: &str, model: Option<String>) {
        let floor = self.function_floors.last().copied().unwrap_or(0);
        if let Some(index) = (floor..self.scopes.len())
            .rev()
            .find(|&index| self.scopes[index].contains_key(name))
        {
            self.scopes[index].insert(name.to_owned(), binding(model));
        } else if self.scopes[..floor]
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
        {
            self.scopes[floor].insert(name.to_owned(), binding(model));
            if let Some(captured) = self.captured_assignments.last_mut() {
                captured.insert(name.to_owned());
            }
        } else if let Some(scope) = self.scopes.get_mut(floor) {
            scope.insert(name.to_owned(), binding(model));
        }
    }

    fn binding(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn model(&self, name: &str) -> Option<&str> {
        match self.binding(name) {
            Some(Binding::Model(model)) => Some(model),
            _ => None,
        }
    }

    fn is_host_context(&self, name: &str) -> bool {
        matches!(self.binding(name), None | Some(Binding::HostContext))
    }

    pub(super) fn declared_host_context(&self, name: &str) -> bool {
        matches!(self.binding(name), Some(Binding::HostContext))
    }
}

fn binding(model: Option<String>) -> Binding {
    model.map_or(Binding::Shadow, Binding::Model)
}

pub(super) fn query_call<'a>(
    call: &'a FunctionCall,
    aliases: &AliasScopes,
) -> Option<(Option<String>, &'a str, &'a FunctionArgs)> {
    let suffixes = call.suffixes().collect::<Vec<_>>();
    query_suffixes(call.prefix(), &suffixes, aliases)
}

pub(super) fn query_var<'a>(
    variable: &'a VarExpression,
    aliases: &AliasScopes,
) -> Option<(Option<String>, &'a str, &'a FunctionArgs)> {
    let suffixes = variable.suffixes().collect::<Vec<_>>();
    query_suffixes(variable.prefix(), &suffixes, aliases)
}

fn query_suffixes<'a>(
    prefix: &'a Prefix,
    suffixes: &[&'a Suffix],
    aliases: &AliasScopes,
) -> Option<(Option<String>, &'a str, &'a FunctionArgs)> {
    suffixes.iter().enumerate().find_map(|(index, suffix)| {
        let Suffix::Call(full_moon::ast::Call::MethodCall(method)) = suffix else {
            return None;
        };
        let method_name = identifier(method.name())?;
        let receiver = &suffixes[..index];
        let model = if receiver.is_empty() {
            let Prefix::Name(name) = prefix else {
                return None;
            };
            identifier(name)
                .and_then(|name| aliases.model(name))
                .map(str::to_owned)
        } else {
            direct_model(prefix, receiver, aliases)
        }?;
        Some((Some(model), method_name, method.args()))
    })
}

pub(super) fn assignment_model(expression: &Expression, aliases: &AliasScopes) -> Option<String> {
    match expression {
        Expression::Parentheses { expression, .. } => assignment_model(expression, aliases),
        Expression::TypeAssertion {
            expression,
            type_assertion,
        } if type_assertion.cast_to().to_string().trim() != "any" => {
            assignment_model(expression, aliases)
        }
        Expression::Var(Var::Name(name)) => identifier(name)
            .and_then(|name| aliases.model(name))
            .map(str::to_owned),
        Expression::Var(Var::Expression(variable)) => {
            let suffixes = variable.suffixes().collect::<Vec<_>>();
            direct_model(variable.prefix(), &suffixes, aliases)
        }
        _ => None,
    }
}

pub(super) fn identifier(token: &TokenReference) -> Option<&str> {
    match token.token().token_type() {
        TokenType::Identifier { identifier } => Some(identifier.as_str()),
        _ => None,
    }
}

fn direct_model(prefix: &Prefix, suffixes: &[&Suffix], aliases: &AliasScopes) -> Option<String> {
    let Prefix::Name(context) = prefix else {
        return None;
    };
    if identifier(context)? != "ctx" || !aliases.is_host_context("ctx") || suffixes.len() != 2 {
        return None;
    }
    if index_name(suffixes[0])? != "models" {
        return None;
    }
    index_name(suffixes[1])
}

fn index_name(suffix: &Suffix) -> Option<String> {
    let Suffix::Index(index) = suffix else {
        return None;
    };
    match index {
        Index::Dot { name, .. } => identifier(name).map(str::to_owned),
        Index::Brackets { expression, .. } => {
            super::literal_string::decoded(expression).map(|(_, value)| value)
        }
        _ => None,
    }
}
