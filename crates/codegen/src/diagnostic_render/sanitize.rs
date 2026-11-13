//! Sanitizes diagnostic text by redacting URIs, credentials, absolute paths, and control characters.

const MAX_TEXT_BYTES: usize = 16 * 1024;

pub(crate) fn exceeds_limit(value: &str) -> bool {
    value.len() > MAX_TEXT_BYTES
}

pub(crate) fn text(value: &str) -> String {
    let mut end = value.len().min(MAX_TEXT_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let stripped = strip_controls(&value[..end]);
    let hidden = hide_tokens(&stripped);
    let mut output = crate::redact_absolute_paths(&hidden);
    if end != value.len() {
        output.push_str("…<truncated>");
    }
    output
}

pub(crate) fn path(value: &str) -> String {
    if std::path::Path::new(value).is_absolute() {
        "<path>".to_owned()
    } else {
        text(value)
    }
}

fn hide_tokens(value: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < value.len() {
        if let Some(end) = hidden_end(value, cursor) {
            output.push_str("<redacted>");
            cursor = end;
        } else {
            let character = value[cursor..].chars().next().unwrap();
            output.push(character);
            cursor += character.len_utf8();
        }
    }
    output
}

fn hidden_end(value: &str, start: usize) -> Option<usize> {
    let tail = &value[start..];
    let first = tail.as_bytes()[0];
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if value[..start]
        .chars()
        .next_back()
        .is_some_and(|part| part.is_ascii_alphanumeric() || part == '_')
    {
        return None;
    }
    let word = tail
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"_-+.".contains(byte))
        .count();
    let after = &tail[word..];
    let uri = after.starts_with("://");
    let credential = [
        "password",
        "passwd",
        "token",
        "secret",
        "authorization",
        "api_key",
        "access_key",
        "credential",
    ]
    .iter()
    .any(|key| tail[..word].eq_ignore_ascii_case(key));
    let after_key = after.trim_start_matches(['\'', '"', '`', ' ', '\t']);
    if !(uri || credential && after_key.starts_with(['=', ':'])) {
        return None;
    }
    let value_start = if uri {
        start + word + 3
    } else {
        let after_separator = after_key[1..].trim_start_matches([' ', '\t', '\'', '"', '`']);
        value.len() - after_separator.len()
    };
    let mut end = value_start;
    let quote = if uri {
        None
    } else {
        value[..value_start]
            .chars()
            .next_back()
            .filter(|c| matches!(c, '\'' | '"' | '`'))
    };
    for character in value[value_start..].chars() {
        if quote.is_some_and(|quote| character == quote)
            || (quote.is_none()
                && (character.is_whitespace() || matches!(character, '\'' | '"' | '`' | '<' | '>')))
        {
            break;
        }
        end += character.len_utf8();
    }
    // Only the space between authorization schemes and credentials is included.
    if credential && tail[..word].eq_ignore_ascii_case("authorization") && quote.is_none() {
        for character in value[end..].chars() {
            if matches!(character, '\r' | '\n' | '\'' | '"' | '`' | ',' | ';') {
                break;
            }
            end += character.len_utf8();
        }
    }
    Some(end.max(start + word))
}

fn strip_controls(value: &str) -> String {
    let mut output = String::new();
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            match characters.next() {
                Some('[') => {
                    for part in characters.by_ref() {
                        if ('@'..='~').contains(&part) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(part) = characters.next() {
                        if part == '\u{7}' || (part == '\u{1b}' && characters.peek() == Some(&'\\'))
                        {
                            if part == '\u{1b}' {
                                characters.next();
                            }
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !character.is_control() || matches!(character, '\n' | '\t') {
            output.push(character);
        }
    }
    output
}
