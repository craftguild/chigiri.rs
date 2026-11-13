use super::{LEX_UNTERMINATED_BLOCK_STRING, ScanError, ScannedToken};
use crate::language::TokenKind;

pub(super) fn scan(source: &str, start: usize) -> ScannedToken {
    let mut position = start + 3;
    let mut raw = String::new();

    while position < source.len() {
        if source[position..].starts_with("\\\"\"\"") {
            raw.push_str("\"\"\"");
            position += 4;
            continue;
        }
        if source[position..].starts_with("\"\"\"") {
            position += 3;
            return ScannedToken::new(TokenKind::BlockString(normalize(&raw)), position, None);
        }
        let Some(character) = source[position..].chars().next() else {
            break;
        };
        raw.push(character);
        position += character.len_utf8();
    }

    ScannedToken::new(
        TokenKind::BlockString(normalize(&raw)),
        source.len(),
        Some(ScanError::new(
            LEX_UNTERMINATED_BLOCK_STRING,
            "Unterminated block string",
            start,
            source.len(),
        )),
    )
}

fn normalize(raw: &str) -> String {
    let normalized = normalize_line_terminators(raw);
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    let common_indent = lines
        .iter()
        .skip(1)
        .filter(|line| !is_blank(line))
        .map(|line| indent_size(line))
        .min();

    if let Some(common_indent) = common_indent {
        for line in lines.iter_mut().skip(1) {
            *line = remove_indent(line, common_indent);
        }
    }
    let first = lines
        .iter()
        .position(|line| !is_blank(line))
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|line| !is_blank(line))
        .map_or(first, |index| index + 1);
    lines[first..last].join("\n")
}

fn normalize_line_terminators(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut characters = raw.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn is_blank(line: &str) -> bool {
    line.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

fn indent_size(line: &str) -> usize {
    line.bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}

fn remove_indent(line: &str, size: usize) -> &str {
    let removable = indent_size(line).min(size);
    line.get(removable..).unwrap_or_default()
}
