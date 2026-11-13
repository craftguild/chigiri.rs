//! Add checker-only terminal returns for lexically proven host `ctx.fail` calls.

use std::collections::{BTreeMap, BTreeSet};

use full_moon::ast::luau::TypeDeclaration;
use full_moon::ast::{
    Assignment, Call, CompoundAssignment, Expression, FunctionCall, FunctionDeclaration, Index,
    Prefix, Suffix, Var,
};
use full_moon::node::Node;
use full_moon::tokenizer::{TokenReference, TokenType};
use full_moon::visitors::Visitor;

use super::literal_precision::ColumnMap;

#[derive(Clone, Copy)]
pub(super) enum Binding {
    HostContext,
    Shadow,
}

#[derive(Default)]
pub(super) struct AliasScopes {
    scopes: Vec<BTreeMap<String, Binding>>,
    function_floors: Vec<usize>,
    reassigned: BTreeSet<String>,
}

impl AliasScopes {
    pub(super) fn with_reassignments(reassigned: BTreeSet<String>) -> Self {
        Self {
            reassigned,
            ..Self::default()
        }
    }

    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn enter_function(&mut self) {
        self.enter_scope();
        self.function_floors.push(self.scopes.len() - 1);
    }

    pub(super) fn exit_function(&mut self) {
        self.function_floors.pop();
        self.exit_scope();
    }

    pub(super) fn declare(&mut self, name: &str, binding: Binding) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), binding);
        }
    }

    pub(super) fn binding(&self, name: &str) -> Binding {
        if self.reassigned.contains(name) {
            return Binding::Shadow;
        }
        let floor = self.function_floors.last().copied().unwrap_or(0);
        self.scopes[floor..]
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .copied()
            .unwrap_or(Binding::Shadow)
    }
}

pub(super) struct Collector {
    pub(super) aliases: AliasScopes,
    pub(super) additions: BTreeMap<usize, &'static str>,
    pub(super) block_bindings: BTreeMap<usize, Vec<String>>,
    pub(super) host_contexts: BTreeSet<String>,
    pub(super) statement_calls: BTreeSet<usize>,
    pub(super) wrapped_calls: BTreeSet<usize>,
}

#[derive(Default)]
struct TypeShadows(BTreeSet<String>);

#[derive(Default)]
struct ReassignedNames(BTreeSet<String>);

impl Visitor for TypeShadows {
    fn visit_type_declaration(&mut self, declaration: &TypeDeclaration) {
        if let Some(name) = identifier(declaration.type_name()) {
            self.0.insert(name.to_owned());
        }
    }
}

impl Visitor for ReassignedNames {
    fn visit_assignment(&mut self, assignment: &Assignment) {
        self.0
            .extend(assignment.variables().iter().filter_map(|variable| {
                let Var::Name(name) = variable else {
                    return None;
                };
                identifier(name).map(str::to_owned)
            }));
    }

    fn visit_compound_assignment(&mut self, assignment: &CompoundAssignment) {
        if let Var::Name(name) = assignment.lhs()
            && let Some(name) = identifier(name)
        {
            self.0.insert(name.to_owned());
        }
    }

    fn visit_function_declaration(&mut self, function: &FunctionDeclaration) {
        let mut names = function.name().names().iter();
        if let (Some(name), None) = (names.next(), names.next())
            && function.name().method_name().is_none()
            && let Some(name) = identifier(name)
        {
            self.0.insert(name.to_owned());
        }
    }
}

pub(super) fn refine(source: &str, columns: &mut ColumnMap, ir: &crate::ir::MockIr) -> String {
    let parsed = full_moon::parse_fallible(source, full_moon::LuaVersion::luau());
    if !parsed.errors().is_empty() {
        return source.to_owned();
    }
    let mut reassigned = ReassignedNames::default();
    reassigned.visit_ast(parsed.ast());
    let host_contexts = host_context_names(ir, parsed.ast());
    let mut collector = Collector {
        aliases: AliasScopes::with_reassignments(reassigned.0),
        additions: BTreeMap::new(),
        block_bindings: BTreeMap::new(),
        host_contexts,
        statement_calls: BTreeSet::new(),
        wrapped_calls: BTreeSet::new(),
    };
    collector.visit_ast(parsed.ast());
    let mut refined = source.to_owned();
    for (byte, addition) in collector.additions.into_iter().rev() {
        columns.add_generated(&refined, byte, addition.len());
        refined.insert_str(byte, addition);
    }
    columns.finish();
    refined
}

pub(super) fn identifier(token: &TokenReference) -> Option<&str> {
    match token.token().token_type() {
        TokenType::Identifier { identifier } => Some(identifier.as_str()),
        _ => None,
    }
}

pub(super) fn host_context_names(
    ir: &crate::ir::MockIr,
    ast: &full_moon::ast::Ast,
) -> BTreeSet<String> {
    let mut shadows = TypeShadows::default();
    shadows.visit_ast(ast);
    let mut names = context_names(ir);
    names.retain(|name| !shadows.0.contains(name));
    names
}

fn context_names(ir: &crate::ir::MockIr) -> BTreeSet<String> {
    let mut names = ir
        .scripts()
        .iter()
        .enumerate()
        .map(|(id, plan)| {
            super::names::resolver(
                ir,
                crate::ir::ScriptId(id),
                if plan.role.is_subscription_root() {
                    "SetupContext"
                } else {
                    "Context"
                },
            )
        })
        .collect::<BTreeSet<_>>();
    names.extend(
        ir.scripts()
            .iter()
            .enumerate()
            .filter(|(_, plan)| plan.role.is_subscription_root())
            .flat_map(|(id, _)| {
                ["ConsumerSetupContext", "ConsumerContext"]
                    .map(|suffix| super::names::resolver(ir, crate::ir::ScriptId(id), suffix))
            }),
    );
    names.extend(
        crate::hooks::HookSlot::ALL
            .into_iter()
            .map(|slot| crate::api::hooks::alias(slot, "Context")),
    );
    names.extend([
        "ChigiriReadContext".to_owned(),
        "ChigiriWriteContext".to_owned(),
    ]);
    names
}

pub(super) fn expression_binding(expression: &Expression, aliases: &AliasScopes) -> Binding {
    match expression {
        Expression::Parentheses { expression, .. } => expression_binding(expression, aliases),
        Expression::TypeAssertion {
            expression,
            type_assertion,
        } if type_assertion.cast_to().to_string().trim() != "any" => {
            expression_binding(expression, aliases)
        }
        Expression::Var(Var::Name(name)) => identifier(name)
            .map(|name| aliases.binding(name))
            .unwrap_or(Binding::Shadow),
        _ => Binding::Shadow,
    }
}

pub(super) fn terminal_call(call: &FunctionCall, aliases: &AliasScopes) -> Option<(usize, usize)> {
    let (start, end) = call.range()?;
    let Prefix::Name(name) = call.prefix() else {
        return None;
    };
    if !matches!(aliases.binding(identifier(name)?), Binding::HostContext) {
        return None;
    }
    let suffixes = call.suffixes().collect::<Vec<_>>();
    let [
        Suffix::Index(Index::Dot { name, .. }),
        Suffix::Call(Call::AnonymousCall(_)),
    ] = suffixes.as_slice()
    else {
        return None;
    };
    (identifier(name) == Some("fail")).then_some((start.bytes(), end.bytes()))
}
