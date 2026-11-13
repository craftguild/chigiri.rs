//! Track host context aliases and standalone calls in lexical statement order.

use full_moon::ast::{
    Block, FunctionBody, FunctionCall, GenericFor, LocalAssignment, LocalFunction, NumericFor,
    Parameter, Stmt,
};
use full_moon::node::Node;
use full_moon::visitors::Visitor;

use super::terminal_flow::{Binding, Collector, expression_binding, identifier, terminal_call};

impl Visitor for Collector {
    fn visit_block(&mut self, block: &Block) {
        let start = block.range().map(|(start, _)| start.bytes());
        self.aliases.enter_scope();
        if let Some(start) = start
            && let Some(names) = self.block_bindings.remove(&start)
        {
            for name in names {
                self.aliases.declare(&name, Binding::Shadow);
            }
        }
        let statements = block.stmts().collect::<Vec<_>>();
        for (index, statement) in statements.iter().enumerate() {
            let Stmt::FunctionCall(call) = statement else {
                continue;
            };
            let Some((call_start, _)) = call.range() else {
                continue;
            };
            self.statement_calls.insert(call_start.bytes());
            if index + 1 < statements.len() || block.last_stmt().is_some() {
                self.wrapped_calls.insert(call_start.bytes());
            }
        }
    }

    fn visit_block_end(&mut self, _: &Block) {
        self.aliases.exit_scope();
    }

    fn visit_function_body(&mut self, body: &FunctionBody) {
        self.aliases.enter_function();
        for (parameter, specifier) in body.parameters().iter().zip(body.type_specifiers()) {
            let Parameter::Name(name) = parameter else {
                continue;
            };
            let Some(name) = identifier(name) else {
                continue;
            };
            let binding = if specifier.is_some_and(|specifier| {
                self.host_contexts
                    .contains(specifier.type_info().to_string().trim())
            }) {
                Binding::HostContext
            } else {
                Binding::Shadow
            };
            self.aliases.declare(name, binding);
        }
    }

    fn visit_function_body_end(&mut self, _: &FunctionBody) {
        self.aliases.exit_function();
    }

    fn visit_local_assignment_end(&mut self, assignment: &LocalAssignment) {
        let bindings = assignment
            .expressions()
            .iter()
            .map(|expression| expression_binding(expression, &self.aliases))
            .collect::<Vec<_>>();
        for (index, name) in assignment.names().iter().enumerate() {
            if let Some(name) = identifier(name) {
                self.aliases.declare(
                    name,
                    bindings.get(index).copied().unwrap_or(Binding::Shadow),
                );
            }
        }
    }

    fn visit_local_function(&mut self, function: &LocalFunction) {
        if let Some(name) = identifier(function.name()) {
            self.aliases.declare(name, Binding::Shadow);
        }
    }

    fn visit_numeric_for(&mut self, loop_: &NumericFor) {
        if let (Some(name), Some((start, _))) =
            (identifier(loop_.index_variable()), loop_.block().range())
        {
            self.block_bindings
                .entry(start.bytes())
                .or_default()
                .push(name.to_owned());
        }
    }

    fn visit_generic_for(&mut self, loop_: &GenericFor) {
        let Some((start, _)) = loop_.block().range() else {
            return;
        };
        self.block_bindings
            .entry(start.bytes())
            .or_default()
            .extend(
                loop_
                    .names()
                    .iter()
                    .filter_map(identifier)
                    .map(str::to_owned),
            );
    }

    fn visit_function_call(&mut self, call: &FunctionCall) {
        let Some((start, _)) = call.range() else {
            return;
        };
        if self.statement_calls.contains(&start.bytes())
            && let Some((start, end)) = terminal_call(call, &self.aliases)
        {
            if self.wrapped_calls.contains(&start) {
                self.additions.insert(start, "do ");
                self.additions
                    .insert(end, "; return ((nil :: any) :: () -> never)() end");
            } else {
                self.additions
                    .insert(end, "; return ((nil :: any) :: () -> never)()");
            }
        }
    }
}
