//! Expands a selection set into runtime-object field groups ordered by response key.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::executable::{CoercedInput, CoercedVariables, ValidatedDocument};
use crate::language::ast::{Directive, Field, Selection, SelectionSet, Value};
use crate::schema::SemanticSchema;

use super::directive::{DirectiveSource, ExecutableDirectiveSite};

pub(crate) struct CollectedField<'a> {
    pub(crate) response_key: &'a str,
    pub(crate) fields: Vec<&'a Field>,
    pub(crate) directives: Vec<DirectiveSource<'a>>,
}

pub(crate) fn collect_fields<'a>(
    schema: &SemanticSchema,
    document: &'a ValidatedDocument,
    selection_sets: &[&'a SelectionSet],
    runtime_type: &str,
    variables: &CoercedVariables,
) -> Vec<CollectedField<'a>> {
    let mut work = Vec::new();
    for selection_set in selection_sets.iter().rev() {
        push_selections(&mut work, selection_set, &[]);
    }
    let mut visited_fragments = BTreeSet::new();
    let mut indices = BTreeMap::<&str, usize>::new();
    let mut collected = Vec::<CollectedField<'a>>::new();
    while let Some((selection, inherited)) = work.pop() {
        match selection {
            Selection::Field(field) if should_include(&field.directives, variables) => {
                let mut directives = inherited;
                extend_directives(
                    &mut directives,
                    &field.directives,
                    ExecutableDirectiveSite::Field {
                        field: field.name.value.clone(),
                    },
                );
                let response_key = field
                    .alias
                    .as_ref()
                    .map_or(field.name.value.as_str(), |alias| alias.value.as_str());
                if let Some(index) = indices.get(response_key).copied() {
                    collected[index].fields.push(field);
                    collected[index].directives.extend(directives);
                } else {
                    indices.insert(response_key, collected.len());
                    collected.push(CollectedField {
                        response_key,
                        fields: vec![field],
                        directives,
                    });
                }
            }
            Selection::Field(_) => {}
            Selection::FragmentSpread(spread)
                if should_include(&spread.directives, variables)
                    && !visited_fragments.contains(spread.name.value.as_str()) =>
            {
                visited_fragments.insert(spread.name.value.as_str());
                if let Some(fragment) = document.fragment(&spread.name.value)
                    && type_condition_applies(schema, &fragment.type_condition.value, runtime_type)
                {
                    let mut directives = inherited;
                    extend_directives(
                        &mut directives,
                        &spread.directives,
                        ExecutableDirectiveSite::FragmentSpread {
                            fragment: spread.name.value.clone(),
                        },
                    );
                    extend_directives(
                        &mut directives,
                        &fragment.directives,
                        ExecutableDirectiveSite::FragmentDefinition {
                            fragment: fragment.name.value.clone(),
                        },
                    );
                    push_selections(&mut work, &fragment.selection_set, &directives);
                }
            }
            Selection::FragmentSpread(_) => {}
            Selection::InlineFragment(fragment)
                if should_include(&fragment.directives, variables)
                    && fragment.type_condition.as_ref().is_none_or(|condition| {
                        type_condition_applies(schema, &condition.value, runtime_type)
                    }) =>
            {
                let mut directives = inherited;
                extend_directives(
                    &mut directives,
                    &fragment.directives,
                    ExecutableDirectiveSite::InlineFragment,
                );
                push_selections(&mut work, &fragment.selection_set, &directives);
            }
            Selection::InlineFragment(_) => {}
        }
    }
    collected
}

fn push_selections<'a>(
    work: &mut Vec<(&'a Selection, Vec<DirectiveSource<'a>>)>,
    selection_set: &'a SelectionSet,
    inherited: &[DirectiveSource<'a>],
) {
    work.extend(
        selection_set
            .selections
            .iter()
            .rev()
            .map(|selection| (selection, inherited.to_vec())),
    );
}

fn extend_directives<'a>(
    output: &mut Vec<DirectiveSource<'a>>,
    directives: &'a [Directive],
    site: ExecutableDirectiveSite,
) {
    let site = Arc::new(site);
    output.extend(directives.iter().map(|directive| DirectiveSource {
        directive,
        site: site.clone(),
    }));
}

fn type_condition_applies(schema: &SemanticSchema, condition: &str, runtime_type: &str) -> bool {
    condition == runtime_type
        || schema
            .possible_objects(condition)
            .is_some_and(|objects| objects.iter().any(|object| object.as_ref() == runtime_type))
}

pub(crate) fn should_include(directives: &[Directive], variables: &CoercedVariables) -> bool {
    for directive in directives {
        let exclude = match directive.name.value.as_str() {
            "skip" => directive_if(directive, variables) == Some(true),
            "include" => directive_if(directive, variables) != Some(true),
            _ => false,
        };
        if exclude {
            return false;
        }
    }
    true
}

fn directive_if(directive: &Directive, variables: &CoercedVariables) -> Option<bool> {
    let argument = directive
        .arguments
        .iter()
        .find(|argument| argument.name.value == "if")?;
    match &argument.value.value {
        Value::Boolean(value) => Some(*value),
        Value::Variable(name) => match variables.get(&name.value) {
            Some(CoercedInput::Value(crate::Value::Boolean(value))) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::ast::{Definition, ExecutableDefinition};

    #[test]
    fn rsm_16_inherited_directive_sites_share_names_across_child_work_items() {
        let name = "F".repeat(4096);
        let text = format!("fragment {name} on Item @d {{{}}}", "v ".repeat(1000));
        let document = crate::language::parse_executable_document(
            crate::SourceId::new(0),
            &text,
            &crate::ExecutionLimits::production(),
        )
        .document
        .unwrap();
        let Definition::Executable(ExecutableDefinition::Fragment(fragment)) =
            &document.definitions[0]
        else {
            panic!()
        };
        let mut sources = Vec::new();
        extend_directives(
            &mut sources,
            &fragment.directives,
            ExecutableDirectiveSite::FragmentDefinition { fragment: name },
        );
        let mut work = Vec::new();
        push_selections(&mut work, &fragment.selection_set, &sources);
        assert_eq!(work.len(), 1000);
        assert!(
            work.iter()
                .all(|(_, inherited)| Arc::ptr_eq(&sources[0].site, &inherited[0].site))
        );
    }
}
