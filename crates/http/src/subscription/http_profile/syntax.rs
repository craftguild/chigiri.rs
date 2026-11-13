//! Borrowed HTTP list and parameter syntax without an input-sized allocation.

/// Splits a list only outside quoted strings, including quoted-pair escapes.
pub(super) struct Pieces<'a> {
    remaining: Option<&'a str>,
    delimiter: u8,
}

impl<'a> Pieces<'a> {
    pub(super) fn new(input: &'a str, delimiter: u8) -> Self {
        Self {
            remaining: Some(input),
            delimiter,
        }
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let input = self.remaining.take()?;
        let mut quoted = false;
        let mut escaped = false;
        for (index, byte) in input.bytes().enumerate() {
            if escaped {
                escaped = false;
            } else if quoted && byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = !quoted;
            } else if !quoted && byte == self.delimiter {
                self.remaining = Some(&input[index + 1..]);
                return Some(&input[..index]);
            }
        }
        Some(input)
    }
}

pub(super) fn trim(input: &str) -> &str {
    input.trim_matches([' ', '\t'])
}

pub(super) fn token(input: &str) -> bool {
    !input.is_empty()
        && input.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

#[derive(Clone, Copy)]
pub(super) struct ParameterValue<'a> {
    bytes: &'a [u8],
    quoted: bool,
}

impl<'a> ParameterValue<'a> {
    pub(super) fn parse(input: &'a str) -> Option<Self> {
        if !input.starts_with('"') {
            return token(input).then_some(Self {
                bytes: input.as_bytes(),
                quoted: false,
            });
        }
        let bytes = input.strip_prefix('"')?.strip_suffix('"')?.as_bytes();
        let mut escaped = false;
        for &byte in bytes {
            if escaped {
                if !matches!(byte, b'\t' | b' '..=b'~' | 0x80..=0xff) {
                    return None;
                }
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if !matches!(
                byte,
                b'\t' | b' ' | b'!' | b'#'..=b'[' | b']'..=b'~' | 0x80..=0xff
            ) {
                return None;
            }
        }
        (!escaped).then_some(Self {
            bytes,
            quoted: true,
        })
    }

    pub(super) fn equals(self, expected: &str, ignore_case: bool) -> bool {
        let mut expected = expected.bytes();
        let mut escaped = false;
        for &byte in self.bytes {
            if self.quoted && byte == b'\\' && !escaped {
                escaped = true;
                continue;
            }
            escaped = false;
            let Some(wanted) = expected.next() else {
                return false;
            };
            if !(byte == wanted || (ignore_case && byte.eq_ignore_ascii_case(&wanted))) {
                return false;
            }
        }
        expected.next().is_none()
    }
}

/// Parses RFC 9110 qvalues as thousandths, without float rounding or padding.
pub(super) fn quality(input: &str) -> Option<u16> {
    let (whole, fraction) = input.split_once('.').unwrap_or((input, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut thousandths = 0_u16;
    let mut scale = 100;
    for digit in fraction.bytes() {
        thousandths += u16::from(digit - b'0') * scale;
        scale /= 10;
    }
    match whole {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(1_000),
        _ => None,
    }
}
