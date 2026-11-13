//! Correlates a selected external build invocation with its primary diagnostic span.

use serde_json::Value;

use super::{Sources, number};

pub(super) fn missing(span: &Value, sources: &Sources, symbol: &str) -> bool {
    let Some((source, start, end)) = source(span, sources) else {
        return false;
    };
    let selected = &source[start..end];
    if !(selected == "build" || selected.starts_with("with_"))
        || !selected
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !source[end..].trim_start().starts_with('(')
    {
        return false;
    }
    let Some(mut tail) = builder(&source[..start], sources, symbol) else {
        return false;
    };
    loop {
        let Some(rest) = tail.trim_start().strip_prefix('.') else {
            return false;
        };
        if rest.trim().is_empty() {
            return true;
        }
        let Some(rest) = method(rest) else {
            return false;
        };
        let Some(rest) = balanced(rest, '(', ')') else {
            return false;
        };
        tail = rest;
    }
}

fn source(span: &Value, sources: &Sources) -> Option<(std::rc::Rc<str>, usize, usize)> {
    let source = sources.read(span.get("file_name")?.as_str()?)?;
    let start = number(span, "byte_start");
    let end = number(span, "byte_end");
    source.get(start..end)?;
    if source.contains("extern crate self") {
        return None;
    }
    Some((source, start, end))
}

fn builder<'a>(source: &'a str, sources: &Sources, symbol: &str) -> Option<&'a str> {
    let tail = [
        format!("::{}::{symbol}", sources.alias),
        format!("::r#{}::{symbol}", sources.alias),
    ]
    .into_iter()
    .filter_map(|prefix| {
        source
            .rfind(&prefix)
            .filter(|start| {
                source[..*start].chars().next_back().is_none_or(|previous| {
                    !previous.is_alphanumeric() && !matches!(previous, '_' | '#' | ':' | '>')
                })
            })
            .map(|start| (start, &source[start + prefix.len()..]))
    })
    .max_by_key(|(start, _)| *start)?
    .1
    .trim_start();
    let tail = if tail.starts_with("::<") {
        balanced(&tail[2..], '<', '>')?
    } else {
        tail
    };
    balanced(tail.trim_start().strip_prefix("::builder")?, '(', ')')
}

fn method(source: &str) -> Option<&str> {
    let length = source.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '#')?;
    (length != 0)
        .then(|| source[length..].trim_start())
        .filter(|tail| tail.starts_with('('))
}

fn balanced(source: &str, open: char, close: char) -> Option<&str> {
    if !source.starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut characters = source.char_indices();
    while let Some((index, character)) = characters.next() {
        if character == '"' {
            // The parentheses in the normal string of a forward callback do not interpret as partitions.
            // Raw string and char/lifetime/comment are excluded from this limited analysis.
            if source[..index].ends_with(['r', '#']) {
                return None;
            }
            let mut closed = false;
            while let Some((_, character)) = characters.next() {
                if character == '\\' {
                    characters.next()?;
                } else if character == '"' {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return None;
            }
            continue;
        }
        if matches!(character, '\'' | '/' | ';') {
            return None;
        }
        if character == open {
            depth += 1;
        }
        if character == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return source.get(index + character.len_utf8()..);
            }
        }
    }
    None
}
