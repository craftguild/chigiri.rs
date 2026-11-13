//! Apply lexical host receiver state while visiting query and source call syntax.

use full_moon::ast::{
    Assignment, Block, Expression, FunctionArgs, FunctionBody, FunctionCall, FunctionDeclaration,
    GenericFor, LocalAssignment, LocalFunction, NumericFor, Parameter, Return, Var, VarExpression,
};
use full_moon::node::Node;
use full_moon::visitors::Visitor;

use super::literal_precision::Collector;
use super::{literal_precision_syntax, literal_precision_types};

impl Visitor for Collector<'_> {
    fn visit_block(&mut self, block: &Block) {
        self.aliases.enter_scope();
        if let Some((start, _)) = block.range()
            && let Some(names) = self.block_bindings.remove(&start.bytes())
        {
            for name in names {
                self.aliases.declare(&name, None);
            }
        }
    }

    fn visit_block_end(&mut self, _: &Block) {
        self.aliases.exit_scope();
    }

    fn visit_function_body(&mut self, body: &FunctionBody) {
        self.aliases.enter_function();
        let return_site = body.return_type().and_then(|specifier| {
            literal_precision_types::function_return(
                self.ir,
                specifier.type_info().to_string().trim(),
            )
        });
        self.return_sites.push(return_site);
        for (parameter, specifier) in body.parameters().iter().zip(body.type_specifiers()) {
            let Parameter::Name(name) = parameter else {
                continue;
            };
            let Some(name) = literal_precision_syntax::identifier(name) else {
                continue;
            };
            let host = specifier.is_some_and(|specifier| {
                let annotation = specifier.type_info().to_string();
                let annotation = annotation.trim();
                self.host_contexts.contains(annotation)
            });
            if host {
                self.aliases.declare_host_context(name);
            } else {
                self.aliases.declare(name, None);
            }
        }
    }

    fn visit_function_body_end(&mut self, _: &FunctionBody) {
        self.return_sites.pop();
        self.aliases.exit_function();
    }

    fn visit_return(&mut self, return_: &Return) {
        let Some(Some(expected)) = self.return_sites.last().cloned() else {
            return;
        };
        for value in return_.returns().iter() {
            self.push_literal(value, &expected, None);
        }
    }

    fn visit_local_assignment(&mut self, assignment: &LocalAssignment) {
        for (specifier, value) in assignment
            .type_specifiers()
            .zip(assignment.expressions().iter())
        {
            let Some(expected) = specifier.and_then(|specifier| {
                literal_precision_types::local(specifier.type_info().to_string().trim())
            }) else {
                continue;
            };
            self.push_literal(value, &expected, None);
        }
    }

    fn visit_local_assignment_end(&mut self, assignment: &LocalAssignment) {
        let models = assignment
            .expressions()
            .iter()
            .map(|value| literal_precision_syntax::assignment_model(value, &self.aliases))
            .collect::<Vec<_>>();
        for (index, name) in assignment.names().iter().enumerate() {
            let Some(name) = literal_precision_syntax::identifier(name) else {
                continue;
            };
            self.aliases
                .declare(name, models.get(index).cloned().flatten());
        }
    }

    fn visit_assignment_end(&mut self, assignment: &Assignment) {
        let models = assignment
            .expressions()
            .iter()
            .map(|value| literal_precision_syntax::assignment_model(value, &self.aliases))
            .collect::<Vec<_>>();
        let changes = assignment
            .variables()
            .iter()
            .enumerate()
            .filter_map(|(index, variable)| {
                let Var::Name(name) = variable else {
                    return None;
                };
                let name = literal_precision_syntax::identifier(name)?.to_owned();
                Some((name, models.get(index).cloned().flatten()))
            })
            .collect::<Vec<_>>();
        for (name, model) in changes {
            self.aliases.assign(&name, model);
        }
    }

    fn visit_local_function(&mut self, function: &LocalFunction) {
        if let Some(name) = literal_precision_syntax::identifier(function.name()) {
            self.aliases.declare(name, None);
        }
    }

    fn visit_function_declaration(&mut self, function: &FunctionDeclaration) {
        let mut names = function.name().names().iter();
        if let (Some(name), None) = (names.next(), names.next())
            && function.name().method_name().is_none()
            && let Some(name) = literal_precision_syntax::identifier(name)
        {
            self.aliases.assign(name, None);
        }
    }

    fn visit_numeric_for(&mut self, loop_: &NumericFor) {
        if let (Some(name), Some((start, _))) = (
            literal_precision_syntax::identifier(loop_.index_variable()),
            loop_.block().range(),
        ) {
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
        let names = loop_
            .names()
            .iter()
            .filter_map(literal_precision_syntax::identifier)
            .map(str::to_owned);
        self.block_bindings
            .entry(start.bytes())
            .or_default()
            .extend(names);
    }

    fn visit_function_call(&mut self, call: &FunctionCall) {
        self.source_input(call.prefix(), &call.suffixes().collect::<Vec<_>>());
        let Some((model, method, arguments)) =
            literal_precision_syntax::query_call(call, &self.aliases)
        else {
            return;
        };
        self.query_options(model.as_deref(), method, arguments);
    }

    fn visit_var_expression(&mut self, variable: &VarExpression) {
        self.source_input(variable.prefix(), &variable.suffixes().collect::<Vec<_>>());
        let Some((model, method, arguments)) =
            literal_precision_syntax::query_var(variable, &self.aliases)
        else {
            return;
        };
        self.query_options(model.as_deref(), method, arguments);
    }
}

impl Collector<'_> {
    fn query_options(&mut self, model: Option<&str>, method: &str, arguments: &FunctionArgs) {
        let Some(model) = model else {
            return;
        };
        let Some(spec) = crate::api::METHODS.iter().find(|spec| spec.name == method) else {
            return;
        };
        let FunctionArgs::Parentheses { arguments, .. } = arguments else {
            return;
        };
        let values = arguments.iter().collect::<Vec<_>>();
        let target = relation_target(self.ir, Some(model), spec, &values);
        for (argument, value) in spec.arguments.iter().zip(values) {
            let option_model = match argument.ty {
                crate::api::TypeExpr::FindOptions | crate::api::TypeExpr::CountOptions => {
                    Some(model)
                }
                crate::api::TypeExpr::TargetOptions => target.as_deref(),
                _ => continue,
            };
            let Expression::TableConstructor(options) = value else {
                continue;
            };
            self.options(option_model, method, argument.ty, options);
        }
        if method == "update"
            && let Some(Expression::TableConstructor(patch)) = arguments.iter().nth(1)
        {
            self.update_patch(model, patch);
        }
    }
}

fn relation_target(
    ir: &crate::ir::MockIr,
    model: Option<&str>,
    spec: &crate::api::MethodSpec,
    values: &[&Expression],
) -> Option<String> {
    let relation = spec
        .arguments
        .iter()
        .zip(values)
        .find_map(|(argument, value)| {
            (argument.ty == crate::api::TypeExpr::Relation)
                .then(|| super::literal_string::decoded(value).map(|(_, name)| name))?
        })?;
    let ty = ir.graph().type_id(model?)?;
    let owner = ir.model_for(ty)?;
    let (_, target) = ir.relation_target(owner, &relation)?;
    Some(
        ir.graph().types()[ir.models()[target.0].ty.0]
            .name
            .to_string(),
    )
}
