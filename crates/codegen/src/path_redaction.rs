//! Redacts absolute paths embedded in diagnostic text.

/// Replaces Unix, Windows, UNC, and file-URI absolute paths with one placeholder.
#[doc(hidden)]
pub fn redact_absolute_paths(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        if let Some(end) = absolute_path_end(value, cursor) {
            output.push_str("<path>");
            cursor = end;
            continue;
        }
        let character = value[cursor..]
            .chars()
            .next()
            .expect("The cursor is within the character boundary.");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn absolute_path_end(value: &str, start: usize) -> Option<usize> {
    let tail = value.get(start..)?;
    if tail
        .get(.."file:".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
    {
        if !file_uri_boundary(value, start) {
            return None;
        }
        return file_uri_end(value, start);
    }
    if !has_path_boundary(value, start) {
        return None;
    }
    let bytes = tail.as_bytes();
    let minimum = if bytes.first() == Some(&b'/') {
        1
    } else if bytes.get(..2) == Some(b"\\\\") {
        2
    } else if bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && bytes
            .get(2)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
    {
        3
    } else {
        return None;
    };
    let quote = value[..start]
        .chars()
        .next_back()
        .filter(|character| matches!(character, '\'' | '"' | '`'));
    let mut end = start + minimum;
    for character in value[end..].chars() {
        if path_token_terminator(character, quote) {
            break;
        }
        end += character.len_utf8();
    }
    Some(trim_terminal_punctuation(value, start + minimum, end))
}

fn has_path_boundary(value: &str, start: usize) -> bool {
    standard_boundary(value, start) || key_path_boundary(value, start)
}

fn standard_boundary(value: &str, start: usize) -> bool {
    value[..start].chars().next_back().is_none_or(|character| {
        character.is_whitespace()
            || matches!(
                character,
                '=' | '(' | '[' | '{' | '<' | ',' | ';' | '\'' | '"' | '`'
            )
    })
}

fn key_path_boundary(value: &str, start: usize) -> bool {
    let Some(key_end) = start.checked_sub(1) else {
        return false;
    };
    if value.as_bytes().get(key_end) != Some(&b':') {
        return false;
    }
    let key_start = value[..key_end]
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            (!key_character(character)).then_some(index + character.len_utf8())
        })
        .unwrap_or(0);
    let Some(key) = value.get(key_start..key_end).filter(|key| !key.is_empty()) else {
        return false;
    };
    // A URI scheme also resembles a key prefix; do not redact `scheme://` as an absolute path.
    if value[start..].starts_with("//") && uri_scheme(key) {
        return false;
    }
    standard_boundary(value, key_start)
}

fn key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
}

fn uri_scheme(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn file_uri_boundary(value: &str, start: usize) -> bool {
    value[..start].chars().next_back().is_none_or(|character| {
        !character.is_alphanumeric() && !matches!(character, '_' | '+' | '-' | '.')
    })
}

fn file_uri_end(value: &str, start: usize) -> Option<usize> {
    let tail = value.get(start..)?;
    if !tail
        .get(.."file:".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
    {
        return None;
    }
    let remainder = tail.get("file:".len()..)?;
    let minimum = if remainder.starts_with("///") {
        "file:///".len()
    } else if let Some(authority) = remainder.strip_prefix("//") {
        let authority_end = authority
            .find(|character: char| character == '/' || path_token_terminator(character, None))
            .unwrap_or(authority.len());
        if authority_end == 0 {
            return None;
        }
        "file://".len()
    } else if remainder.starts_with('/') {
        "file:/".len()
    } else {
        return None;
    };
    let quote = value[..start]
        .chars()
        .next_back()
        .filter(|character| matches!(character, '\'' | '"' | '`'));
    let mut end = start + minimum;
    for character in value[end..].chars() {
        if path_token_terminator(character, quote) {
            break;
        }
        end += character.len_utf8();
    }
    Some(trim_terminal_punctuation(value, start + minimum, end))
}

fn trim_terminal_punctuation(value: &str, minimum: usize, mut end: usize) -> usize {
    while end > minimum
        && value[..end].chars().next_back().is_some_and(|character| {
            matches!(
                character,
                '.' | '!' | '?' | ',' | ';' | ':' | ')' | ']' | '}' | '>' | '|'
            )
        })
    {
        end -= value[..end]
            .chars()
            .next_back()
            .expect("the trailing character was already validated")
            .len_utf8();
    }
    end
}

fn path_token_terminator(character: char, quote: Option<char>) -> bool {
    if let Some(quote) = quote {
        return character == quote || matches!(character, '\r' | '\n');
    }
    character.is_whitespace() || matches!(character, '\'' | '"' | '`')
}

#[cfg(test)]
mod tests {
    use super::redact_absolute_paths;

    #[test]
    fn redacts_empty_boundary_and_primary_absolute_path_forms() {
        let input = concat!(
            "path=/outside/secret.rs ",
            "uri=file:///outside/secret.rs ",
            "drive_uri=file:///C:/Users/alice/secret.rs. ",
            "authority='file://server/share/secret.rs', ",
            "ipv6=file://[::1]/share/secret.rs; ",
            "comma=file:///C:/Users/alice/a,b/secret.rs, ",
            "embedded=see:file:///C:/Users/alice/secret.rs! ",
            "unix_comma=/tmp/user,name/secret.rs ",
            "unix_colon=/tmp/user:group/secret.rs ",
            "source:/outside/source-secret.rs ",
            "cwd:C:\\Users\\alice\\secret.rs ",
            "drive=Z:\\outside\\secret.rs ",
            "unc=\\\\server\\share\\secret.rs\n",
            "quoted=\"/outside/with space/secret.rs\", ",
            "colon=path:/absolute/path! ",
            "punctuation=(/outside/secret.rs);",
        );
        let output = redact_absolute_paths(input);
        for secret in [
            "/outside",
            "file:///",
            "C:/Users",
            "server/share",
            "[::1]",
            "a,b",
            "user,name",
            "user:group",
            "/absolute/path",
            "Z:\\",
            "\\\\server",
            "secret.rs",
        ] {
            assert!(!output.contains(secret), "secret path was not redacted");
        }
        assert!(output.contains("path=<path>"));
        assert!(output.contains("quoted=\"<path>\""));
        assert!(output.contains("drive_uri=<path>."));
        assert!(output.contains("authority='<path>',"));
        assert!(output.contains("ipv6=<path>;"));
        assert!(output.contains("comma=<path>,"));
        assert!(output.contains("embedded=see:<path>!"));
        assert!(output.contains("colon=path:<path>!"));
        assert!(output.contains("source:<path>"));
        assert!(output.contains("cwd:<path>"));
    }

    #[test]
    fn preserves_relative_paths_and_safe_urls() {
        let input = concat!(
            "src/lib.rs https://example.com/a,b:c;d C:relative ",
            "file:relative classpath:resource notpath:resource ",
            "not-file:///resource custom.file:///resource plus+file:///resource",
        );
        assert_eq!(redact_absolute_paths(input), input);
    }
}
