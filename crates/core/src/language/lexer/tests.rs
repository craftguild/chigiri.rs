use super::{LEX_INVALID_CHARACTER, LEX_INVALID_NUMBER, lex};
use crate::language::TokenKind;
use crate::{DiagnosticCode, SourceId};

fn kinds(source: &str) -> Vec<TokenKind> {
    let output = lex(SourceId::new(0), source, 100);
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.tokens.into_iter().map(|token| token.kind).collect()
}

fn first_code(source: &str) -> DiagnosticCode {
    let output = lex(SourceId::new(0), source, 100);
    assert!(output.had_errors);
    output.diagnostics.iter().next().unwrap().code
}

#[test]
fn discards_ignored_tokens_and_crlf() {
    let source = "\u{FEFF}\t, # ignored comment.\r\nquery\u{FEFF},Name # EOF";
    assert_eq!(
        kinds(source),
        [
            TokenKind::Name("query".to_owned()),
            TokenKind::Name("Name".to_owned()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn treats_standalone_cr_and_lf_as_line_terminators() {
    assert_eq!(
        kinds("a\rb\nc"),
        [
            TokenKind::Name("a".to_owned()),
            TokenKind::Name("b".to_owned()),
            TokenKind::Name("c".to_owned()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn recognizes_punctuators_and_spread_tokens() {
    assert_eq!(
        kinds("! $ & ( ) ... : = @ [ ] { | }"),
        [
            TokenKind::Bang,
            TokenKind::Dollar,
            TokenKind::Ampersand,
            TokenKind::ParenLeft,
            TokenKind::ParenRight,
            TokenKind::Spread,
            TokenKind::Colon,
            TokenKind::Equals,
            TokenKind::At,
            TokenKind::BracketLeft,
            TokenKind::BracketRight,
            TokenKind::BraceLeft,
            TokenKind::Pipe,
            TokenKind::BraceRight,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn ascii_names_use_maximal_munch() {
    assert_eq!(
        kinds("_a1 Query query QUERY"),
        [
            TokenKind::Name("_a1".to_owned()),
            TokenKind::Name("Query".to_owned()),
            TokenKind::Name("query".to_owned()),
            TokenKind::Name("QUERY".to_owned()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn rejects_unicode_outside_names_and_preserves_the_byte_span() {
    let output = lex(SourceId::new(3), "query €", 100);
    let diagnostic = output.diagnostics.iter().next().unwrap();

    assert_eq!(diagnostic.code, LEX_INVALID_CHARACTER);
    assert_eq!(diagnostic.span.unwrap().start, 6);
    assert_eq!(diagnostic.span.unwrap().end, 9);
}

#[test]
fn accepts_only_ascii_whitespace() {
    for source in ["query\u{00A0}Name", "query\u{2003}Name"] {
        assert_eq!(first_code(source), LEX_INVALID_CHARACTER, "{source:?}");
    }
}

#[test]
fn ignores_bom_outside_strings_and_treats_it_as_a_comment_boundary() {
    assert_eq!(
        kinds("query # comment\u{FEFF} still comment\nfield"),
        [
            TokenKind::Name("query".to_owned()),
            TokenKind::Name("field".to_owned()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn integer_and_float_tokens_preserve_source_lexemes() {
    assert_eq!(
        kinds("0 -0 123 -45 1.0 -2.5e+3 6E2"),
        [
            TokenKind::Int("0".to_owned()),
            TokenKind::Int("-0".to_owned()),
            TokenKind::Int("123".to_owned()),
            TokenKind::Int("-45".to_owned()),
            TokenKind::Float("1.0".to_owned()),
            TokenKind::Float("-2.5e+3".to_owned()),
            TokenKind::Float("6E2".to_owned()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn rejects_leading_zeroes() {
    assert_eq!(first_code("01"), LEX_INVALID_NUMBER);
    assert_eq!(first_code("-01"), LEX_INVALID_NUMBER);
}

#[test]
fn rejects_non_graphql_integer_representations() {
    for source in ["00", "0x123", "123L"] {
        assert_eq!(first_code(source), LEX_INVALID_NUMBER, "{source}");
    }
}

#[test]
fn requires_digits_in_fraction_and_exponent_parts() {
    for source in ["1.", "1e", "1E+", "-2e-"] {
        assert_eq!(first_code(source), LEX_INVALID_NUMBER, "{source}");
    }
}

#[test]
fn rejects_invalid_number_suffixes_and_extra_decimal_points() {
    for source in ["1.2.3", "1.2foo", "1foo", "0_x"] {
        assert_eq!(first_code(source), LEX_INVALID_NUMBER, "{source}");
    }
}

#[test]
fn adjacent_names_report_an_error_without_joining_the_numeric_lexeme() {
    let output = lex(SourceId::new(0), "123abc", 100);

    assert_eq!(output.tokens[0].kind, TokenKind::Int("123".to_owned()));
    assert_eq!(output.tokens[0].span.start, 0);
    assert_eq!(output.tokens[0].span.end, 3);
    assert_eq!(output.tokens[1].kind, TokenKind::Name("abc".to_owned()));
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        LEX_INVALID_NUMBER
    );
}

#[test]
fn separates_a_float_from_an_adjacent_name() {
    let output = lex(SourceId::new(0), "1.25e2name", 100);

    assert_eq!(output.tokens[0].kind, TokenKind::Float("1.25e2".to_owned()));
    assert_eq!(output.tokens[1].kind, TokenKind::Name("name".to_owned()));
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        LEX_INVALID_NUMBER
    );
}

#[test]
fn spread_token_uses_the_longest_match() {
    let output = lex(SourceId::new(0), "....", 100);

    assert_eq!(output.tokens[0].kind, TokenKind::Spread);
    assert_eq!(output.tokens[0].span.end, 3);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        LEX_INVALID_CHARACTER
    );
}
