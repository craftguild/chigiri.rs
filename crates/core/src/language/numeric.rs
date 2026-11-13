//! Validates GraphQL numeric lexemes in programmatically constructed AST values.

pub(crate) fn valid_int(raw: &str) -> bool {
    integer_prefix(raw.as_bytes()) == Some(raw.len())
}

pub(crate) fn valid_float(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let Some(mut index) = integer_prefix(bytes) else {
        return false;
    };
    let mut has_fraction_or_exponent = false;
    if bytes.get(index) == Some(&b'.') {
        has_fraction_or_exponent = true;
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        has_fraction_or_exponent = true;
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    has_fraction_or_exponent && index == bytes.len()
}

fn integer_prefix(bytes: &[u8]) -> Option<usize> {
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index)? {
        b'0' => Some(index + 1),
        b'1'..=b'9' => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            Some(index)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{valid_float, valid_int};

    #[test]
    fn parses_graphql_numbers_consistently() {
        assert!(valid_int("-0"));
        assert!(valid_float("1.0e+2"));
        assert!(!valid_int("01"));
        assert!(!valid_int("+1"));
        assert!(!valid_float("1"));
        assert!(!valid_float("1.e2"));
    }
}
