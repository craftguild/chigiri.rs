//! Lexical analysis for the September 2025 GraphQL specification.

mod block_string;
mod number;
mod string;

use super::{Token, TokenKind};
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, SourceId, Span};

/// Source text contains a character that cannot begin a GraphQL token.
pub const LEX_INVALID_CHARACTER: DiagnosticCode = DiagnosticCode::new("SF1101");
/// The number literal does not match the IntValue / FloatValue grammar.
pub const LEX_INVALID_NUMBER: DiagnosticCode = DiagnosticCode::new("SF1102");
/// A quoted string ended before its closing quote.
pub const LEX_UNTERMINATED_STRING: DiagnosticCode = DiagnosticCode::new("SF1103");
/// A string or block string contains a disallowed source character.
pub const LEX_INVALID_STRING_CHARACTER: DiagnosticCode = DiagnosticCode::new("SF1104");
/// A string contains an undefined escape sequence.
pub const LEX_INVALID_ESCAPE: DiagnosticCode = DiagnosticCode::new("SF1105");
/// Unicode escape does not represent the Unicode scalar value.
pub const LEX_INVALID_UNICODE_ESCAPE: DiagnosticCode = DiagnosticCode::new("SF1106");
/// The block string ends before the final triple quote.
pub const LEX_UNTERMINATED_BLOCK_STRING: DiagnosticCode = DiagnosticCode::new("SF1107");

/// Lexer token sequence and bounded diagnostics.
#[derive(Clone, Debug)]
pub struct LexOutput {
    /// Tokens in source order, ending with exactly one `Eof`.
    pub tokens: Vec<Token>,
    /// Lexical diagnostics retained up to the configured limit.
    pub diagnostics: DiagnosticBag,
    /// Remains true when input had errors even if the diagnostic limit is zero.
    pub had_errors: bool,
}

impl LexOutput {
    /// Returns whether the parser may safely consume the token sequence.
    ///
    /// When false, the tokens are a recovery-only partial result and must not be
    /// passed to the parser as a valid document.
    pub const fn is_valid(&self) -> bool {
        !self.had_errors
    }
}

/// Converts UTF-8 source text to the token sequence shared by SDL and
/// executable documents.
///
/// Stops searching for additional errors after `max_diagnostics` is reached.
/// An `Eof` token is always emitted at the stopping position so untrusted input
/// cannot deprive the parser of a terminator.
pub fn lex(source_id: SourceId, source: &str, max_diagnostics: usize) -> LexOutput {
    Lexer::new(source_id, source, max_diagnostics).run()
}

struct Lexer<'a> {
    source_id: SourceId,
    source: &'a str,
    position: usize,
    tokens: Vec<Token>,
    diagnostics: DiagnosticBag,
    had_errors: bool,
    stopped: bool,
}

impl<'a> Lexer<'a> {
    fn new(source_id: SourceId, source: &'a str, max_diagnostics: usize) -> Self {
        Self {
            source_id,
            source,
            position: 0,
            tokens: Vec::new(),
            diagnostics: DiagnosticBag::new(max_diagnostics),
            had_errors: false,
            stopped: false,
        }
    }

    fn run(mut self) -> LexOutput {
        while self.position < self.source.len() && !self.stopped {
            self.skip_ignored();
            if self.position >= self.source.len() {
                break;
            }
            self.scan_next();
        }
        let eof = Span::new(self.source_id, self.position, self.position);
        self.tokens.push(Token::new(TokenKind::Eof, eof));
        LexOutput {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
            had_errors: self.had_errors,
        }
    }

    fn skip_ignored(&mut self) {
        let bytes = self.source.as_bytes();
        while self.position < bytes.len() {
            match bytes[self.position] {
                b'\t' | b' ' | b',' | b'\n' => self.position += 1,
                b'\r' => {
                    self.position += 1;
                    if bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                }
                b'#' => {
                    self.position += 1;
                    while self.position < bytes.len()
                        && !matches!(bytes[self.position], b'\r' | b'\n')
                    {
                        self.position += 1;
                    }
                }
                0xEF if bytes.get(self.position..self.position + 3)
                    == Some([0xEF, 0xBB, 0xBF].as_slice()) =>
                {
                    self.position += 3;
                }
                _ => break,
            }
        }
    }

    fn scan_next(&mut self) {
        let start = self.position;
        let byte = self.source.as_bytes()[start];
        if let Some(kind) = punctuation(byte) {
            self.push_simple(kind, start + 1);
        } else if byte == b'.' && self.source[start..].starts_with("...") {
            self.push_simple(TokenKind::Spread, start + 3);
        } else if is_name_start(byte) {
            self.scan_name();
        } else if byte.is_ascii_digit()
            || (byte == b'-'
                && self
                    .source
                    .as_bytes()
                    .get(start + 1)
                    .is_some_and(u8::is_ascii_digit))
        {
            self.push_scanned(number::scan(self.source, start));
        } else if byte == b'"' && self.source[start..].starts_with("\"\"\"") {
            self.push_scanned(block_string::scan(self.source, start));
        } else if byte == b'"' {
            self.push_scanned(string::scan(self.source, start));
        } else {
            self.invalid_character();
        }
    }

    fn scan_name(&mut self) {
        let start = self.position;
        self.position += 1;
        while self
            .source
            .as_bytes()
            .get(self.position)
            .is_some_and(|byte| is_name_continue(*byte))
        {
            self.position += 1;
        }
        let value = self.source.get(start..self.position).unwrap_or_default();
        self.tokens.push(Token::new(
            TokenKind::Name(value.to_owned()),
            Span::new(self.source_id, start, self.position),
        ));
    }

    fn invalid_character(&mut self) {
        let start = self.position;
        let width = self.source[start..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        self.position = (start + width).min(self.source.len());
        self.report(ScanError::new(
            LEX_INVALID_CHARACTER,
            "Unexpected character in GraphQL source",
            start,
            self.position,
        ));
    }

    fn push_simple(&mut self, kind: TokenKind, end: usize) {
        let start = self.position;
        self.position = end;
        self.tokens
            .push(Token::new(kind, Span::new(self.source_id, start, end)));
    }

    fn push_scanned(&mut self, scanned: ScannedToken) {
        let start = self.position;
        self.position = scanned.end;
        self.tokens.push(Token::new(
            scanned.kind,
            Span::new(self.source_id, start, scanned.end),
        ));
        if let Some(error) = scanned.error {
            self.report(error);
        }
    }

    fn report(&mut self, error: ScanError) {
        self.had_errors = true;
        let diagnostic = Diagnostic::new(
            error.code,
            error.message,
            Span::new(self.source_id, error.start, error.end),
        );
        let stored = self.diagnostics.push(diagnostic);
        if !stored || self.diagnostics.is_full() {
            self.stopped = true;
        }
    }
}

fn punctuation(byte: u8) -> Option<TokenKind> {
    Some(match byte {
        b'!' => TokenKind::Bang,
        b'$' => TokenKind::Dollar,
        b'&' => TokenKind::Ampersand,
        b'(' => TokenKind::ParenLeft,
        b')' => TokenKind::ParenRight,
        b':' => TokenKind::Colon,
        b'=' => TokenKind::Equals,
        b'@' => TokenKind::At,
        b'[' => TokenKind::BracketLeft,
        b']' => TokenKind::BracketRight,
        b'{' => TokenKind::BraceLeft,
        b'|' => TokenKind::Pipe,
        b'}' => TokenKind::BraceRight,
        _ => return None,
    })
}

pub(super) const fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(super) const fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

pub(super) struct ScannedToken {
    kind: TokenKind,
    end: usize,
    error: Option<ScanError>,
}

impl ScannedToken {
    const fn new(kind: TokenKind, end: usize, error: Option<ScanError>) -> Self {
        Self { kind, end, error }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScanError {
    code: DiagnosticCode,
    message: &'static str,
    start: usize,
    end: usize,
}

impl ScanError {
    const fn new(code: DiagnosticCode, message: &'static str, start: usize, end: usize) -> Self {
        Self {
            code,
            message,
            start,
            end,
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_security;
#[cfg(test)]
mod tests_string;
