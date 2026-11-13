//! Locate a containing brace using bounded lexical delimiters, not Rust item/type parsing.

/// Ordinary strings/comments may contain braces. Skip them before comparing
/// scope; unfamiliar literal syntax and excessive nesting deliberately decline.
pub(super) fn enclosing_brace(source: &str, position: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut braces = Vec::new();
    let mut index = 0;
    while index < position {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += source[index..].find('\n').unwrap_or(bytes.len() - index);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = comment(bytes, index)?;
            }
            b'r' if raw_string(&bytes[index + 1..]) => return None,
            b'"' => index = quoted(bytes, index, b'"')?,
            b'\'' => {
                let next = source.get(index + 1..)?.chars().next()?;
                if next == '\\' || bytes.get(index + 1 + next.len_utf8()) == Some(&b'\'') {
                    index = quoted(bytes, index, b'\'')?;
                } else {
                    // A lifetime/label is not a character literal.
                    index += 1;
                }
            }
            b'{' => {
                if braces.len() == 64 {
                    return None;
                }
                braces.push(index);
                index += 1;
            }
            b'}' => {
                braces.pop()?;
                index += 1;
            }
            _ => index += 1,
        }
        if index > position {
            return None;
        }
    }
    braces.last().copied()
}

fn raw_string(tail: &[u8]) -> bool {
    tail.iter().copied().find(|byte| *byte != b'#') == Some(b'"')
}

fn quoted(bytes: &[u8], start: usize, quote: u8) -> Option<usize> {
    let mut index = start + 1;
    while let Some(&byte) = bytes.get(index) {
        if byte == quote {
            return Some(index + 1);
        }
        if byte == b'\\' {
            index += 1;
        }
        index += 1;
    }
    None
}

fn comment(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        match &bytes[index..index + 2] {
            b"/*" => {
                if depth == 64 {
                    return None;
                }
                depth += 1;
                index += 2;
            }
            b"*/" => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}
