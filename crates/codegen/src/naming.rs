//! Convert GraphQL names and type references to canonical generated Rust forms.

use std::collections::BTreeMap;

use chigiri_core::language::ast::{Name, TypeRef, TypeRefKind};
use chigiri_core::{Diagnostic, DiagnosticCode};

/// Names collide after normalization into the same Rust identifier scope.
pub(crate) const NAME_COLLISION: DiagnosticCode = DiagnosticCode::new("SF4006");
/// Stringifying a type reference exceeded the caller's nesting limit.
pub(crate) const TYPE_REF_NESTING_LIMIT: DiagnosticCode = DiagnosticCode::new("SF4007");

/// Rust identifier case that corresponds to the role of generated item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdentifierCase {
    /// Case for types, enum variants, and related type names.
    UpperCamel,
    /// Case for fields, arguments, and input fields.
    Snake,
}

/// Converts a GraphQL name to the requested case and escapes Rust keywords.
pub(crate) fn rust_identifier(name: &str, case: IdentifierCase) -> String {
    let converted = match case {
        IdentifierCase::UpperCamel => upper_camel_case(name),
        IdentifierCase::Snake => snake_case(name),
    };
    escape_case(converted, case)
}

fn escape_case(converted: String, case: IdentifierCase) -> String {
    if case == IdentifierCase::UpperCamel && is_unrawable_keyword(&converted) {
        let suffix = if converted == "_" {
            "Underscore"
        } else {
            converted.as_str()
        };
        format!("__ChigiriName{suffix}")
    } else {
        raw_identifier(converted)
    }
}

/// Converts a GraphQL name to `UpperCamelCase` for types and variants.
pub(crate) fn upper_camel_case(name: &str) -> String {
    convert_case(name, IdentifierCase::UpperCamel)
}

/// Converts a GraphQL name to `snake_case` for fields and arguments.
pub(crate) fn snake_case(name: &str) -> String {
    convert_case(name, IdentifierCase::Snake)
}

/// Escapes Rust keywords as raw identifiers when possible.
pub(crate) fn raw_identifier(identifier: String) -> String {
    if is_unrawable_keyword(&identifier) {
        let suffix = if identifier == "_" {
            "underscore"
        } else {
            identifier.as_str()
        };
        format!("__chigiri_name_{suffix}")
    } else if is_rust_keyword(&identifier) {
        format!("r#{identifier}")
    } else {
        identifier
    }
}

/// Prefixes a field method while preserving leading underscores and escaping keywords.
pub(crate) fn prefixed_field_method(prefix: &str, name: &str) -> String {
    let snake = snake_case(name);
    let body = snake.trim_start_matches('_');
    let underscores = snake.len() - body.len();
    if underscores == 0 {
        raw_identifier(format!("{prefix}{snake}"))
    } else {
        // Encode the underscore count to avoid adjacent-underscore lint and collisions.
        raw_identifier(format!("{prefix}{underscores}u_{body}"))
    }
}

/// Reserves converted identifiers within one Rust namespace.
#[derive(Default)]
pub(crate) struct IdentifierScope {
    originals: BTreeMap<String, String>,
}

impl IdentifierScope {
    /// Converts and reserves a name, reporting `SF4006` if another GraphQL name collides.
    pub(crate) fn reserve(
        &mut self,
        name: &Name,
        case: IdentifierCase,
    ) -> Result<String, Diagnostic> {
        let converted = match case {
            IdentifierCase::UpperCamel => upper_camel_case(&name.value),
            IdentifierCase::Snake => snake_case(&name.value),
        };
        let identifier = escape_case(converted, case);
        if let Some(original) = self.originals.get(&identifier) {
            if original != &name.value {
                return Err(Diagnostic::new(
                    NAME_COLLISION,
                    format!(
                        "GraphQL name `{}` maps to Rust identifier `{identifier}`, which conflicts with `{original}` in the same scope.",
                        name.value
                    ),
                    name.span,
                ));
            }
        } else {
            self.originals
                .insert(identifier.clone(), name.value.clone());
        }
        Ok(identifier)
    }
}

/// Convert the AST TypeRef to a canonical GraphQL expression within the nesting limit.
///
/// Public AST values may contain deeply nested repeated wrappers. Process them
/// iteratively to avoid exhausting the Rust call stack.
pub(crate) fn canonical_type_ref(
    type_ref: &TypeRef,
    max_nesting: usize,
) -> Result<String, Diagnostic> {
    enum Task<'a> {
        Visit(&'a TypeRef, usize),
        Token(char),
    }

    let mut output = String::new();
    let mut tasks = vec![Task::Visit(type_ref, 0)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Token(token) => output.push(token),
            Task::Visit(current, depth) => {
                if depth > max_nesting {
                    return Err(Diagnostic::new(
                        TYPE_REF_NESTING_LIMIT,
                        format!("TypeRef nesting exceeds the limit of {max_nesting}"),
                        current.span,
                    ));
                }
                match &current.kind {
                    TypeRefKind::Named(name) => output.push_str(&name.value),
                    TypeRefKind::List(item) => {
                        output.push('[');
                        tasks.push(Task::Token(']'));
                        tasks.push(Task::Visit(item, depth.saturating_add(1)));
                    }
                    TypeRefKind::NonNull(nullable) => {
                        tasks.push(Task::Token('!'));
                        tasks.push(Task::Visit(nullable, depth.saturating_add(1)));
                    }
                }
            }
        }
    }
    Ok(output)
}

fn convert_case(name: &str, case: IdentifierCase) -> String {
    let leading_underscores = name.bytes().take_while(|byte| *byte == b'_').count();
    let mut output = "_".repeat(leading_underscores);
    let body = &name[leading_underscores..];
    let words = split_words(body);
    for (index, word) in words.iter().enumerate() {
        match case {
            IdentifierCase::UpperCamel => push_upper_camel_word(&mut output, word),
            IdentifierCase::Snake => {
                if index > 0 {
                    output.push('_');
                }
                output.extend(word.chars().map(|character| character.to_ascii_lowercase()));
            }
        }
    }
    output
}

fn split_words(name: &str) -> Vec<String> {
    let characters = name.chars().collect::<Vec<_>>();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, character) in characters.iter().copied().enumerate() {
        if character == '_' {
            push_word(&mut words, &mut current);
            continue;
        }
        let previous = current.chars().last();
        let next = characters.get(index + 1).copied();
        let starts_word = character.is_ascii_uppercase()
            && previous.is_some_and(|value| {
                value.is_ascii_lowercase()
                    || value.is_ascii_digit()
                    || (value.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
        if starts_word {
            push_word(&mut words, &mut current);
        }
        current.push(character);
    }
    push_word(&mut words, &mut current);
    words
}

fn push_word(words: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

fn push_upper_camel_word(output: &mut String, word: &str) {
    let mut characters = word.chars();
    if let Some(first) = characters.next() {
        output.push(first.to_ascii_uppercase());
        output.extend(characters.map(|character| character.to_ascii_lowercase()));
    }
}

fn is_rust_keyword(identifier: &str) -> bool {
    matches!(
        identifier,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

fn is_unrawable_keyword(identifier: &str) -> bool {
    identifier == "_" || matches!(identifier, "Self" | "crate" | "self" | "super")
}

#[cfg(test)]
mod tests;
