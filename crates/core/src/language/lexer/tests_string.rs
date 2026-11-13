use super::{
    LEX_INVALID_ESCAPE, LEX_INVALID_STRING_CHARACTER, LEX_INVALID_UNICODE_ESCAPE,
    LEX_UNTERMINATED_BLOCK_STRING, LEX_UNTERMINATED_STRING, lex,
};
use crate::language::TokenKind;
use crate::{DiagnosticCode, SourceId};

fn one_kind(source: &str) -> TokenKind {
    let output = lex(SourceId::new(0), source, 100);
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    assert_eq!(output.tokens.len(), 2);
    output.tokens.into_iter().next().unwrap().kind
}

fn first_code(source: &str) -> DiagnosticCode {
    let output = lex(SourceId::new(0), source, 100);
    assert!(output.had_errors);
    output.diagnostics.iter().next().unwrap().code
}

#[test]
fn decodes_escapes_in_regular_strings() {
    assert_eq!(
        one_kind(r#""\"\\\/\b\f\n\r\t""#),
        TokenKind::String("\"\\/\u{0008}\u{000C}\n\r\t".to_owned())
    );
}

#[test]
fn evaluates_unicode_escapes_and_surrogate_pairs() {
    assert_eq!(
        one_kind(r#""A\u0042\u{1F4A9}\uD83D\uDCA9€""#),
        TokenKind::String("AB💩💩€".to_owned())
    );
}

#[test]
fn validates_unicode_escape_scalar_boundaries() {
    assert_eq!(
        one_kind(r#""\u{D7FF}\u{E000}\u{10FFFF}""#),
        TokenKind::String("\u{D7FF}\u{E000}\u{10FFFF}".to_owned())
    );
    assert_eq!(first_code(r#""\u{D800}""#), LEX_INVALID_UNICODE_ESCAPE);
}

#[test]
fn regular_strings_accept_unicode_scalars_except_line_terminators() {
    let value = "a\u{0000}\u{0007}\u{000B}\u{000C}\t\u{007F}\u{0085}λ";
    assert_eq!(
        one_kind(&format!("\"{value}\"")),
        TokenKind::String(value.to_owned())
    );
}

#[test]
fn quoted_strings_preserve_bom_characters() {
    assert_eq!(
        one_kind("\"before\u{FEFF}after\""),
        TokenKind::String("before\u{FEFF}after".to_owned())
    );
}

#[test]
fn empty_block_strings_are_not_split_into_multiple_tokens() {
    assert_eq!(
        one_kind("\"\"\"\"\"\""),
        TokenKind::BlockString(String::new())
    );
}

#[test]
fn normalizes_common_indentation_and_crlf_in_block_strings() {
    let source = "\"\"\"\r\n  Hello,\r\n    World!\r\n\r\n  End\r\n\"\"\"";
    assert_eq!(
        one_kind(source),
        TokenKind::BlockString("Hello,\n  World!\n\nEnd".to_owned())
    );
}

#[test]
fn block_strings_normalize_carriage_returns_with_tab_indentation() {
    let source = "\"\"\"\r\tfirst\r\t\tsecond\r\"\"\"";
    assert_eq!(
        one_kind(source),
        TokenKind::BlockString("first\n\tsecond".to_owned())
    );
}

#[test]
fn blank_block_string_lines_normalize_to_empty_text() {
    assert_eq!(
        one_kind("\"\"\"\n \t\n\t\n\"\"\""),
        TokenKind::BlockString(String::new())
    );
}

#[test]
fn block_string_first_line_does_not_set_common_indentation() {
    assert_eq!(
        one_kind("\"\"\"first\n  second\"\"\""),
        TokenKind::BlockString("first\nsecond".to_owned())
    );
}

#[test]
fn block_strings_only_decode_escaped_triple_quotes() {
    assert_eq!(
        one_kind("\"\"\"a\\\"\"\"b\\n\"\"\""),
        TokenKind::BlockString("a\"\"\"b\\n".to_owned())
    );
}

#[test]
fn rejects_undefined_escapes() {
    assert_eq!(first_code(r#""\q""#), LEX_INVALID_ESCAPE);
}

#[test]
fn rejects_out_of_range_and_malformed_unicode_escapes() {
    for source in [
        r#""\uDEAD""#,
        r#""\u{110000}""#,
        r#""\u{}""#,
        r#""\u12""#,
        r#""\u{12x}""#,
    ] {
        assert_eq!(first_code(source), LEX_INVALID_UNICODE_ESCAPE, "{source}");
    }
}

#[test]
fn rejects_invalid_surrogate_pairs() {
    for source in [r#""\uD83D""#, r#""\uD83D\u0041""#, r#""\uDCA9""#] {
        assert_eq!(first_code(source), LEX_INVALID_UNICODE_ESCAPE, "{source}");
    }
}

#[test]
fn rejects_line_terminators_in_regular_strings() {
    assert_eq!(first_code("\"a\nb\""), LEX_INVALID_STRING_CHARACTER);
    assert_eq!(first_code("\"a\rb\""), LEX_INVALID_STRING_CHARACTER);
}

#[test]
fn block_strings_preserve_all_unicode_scalars_directly() {
    let value = "a\u{0000}\u{0007}\u{007F}\u{0085}b";
    assert_eq!(
        one_kind(&format!("\"\"\"{value}\"\"\"")),
        TokenKind::BlockString(value.to_owned())
    );
}

#[test]
fn diagnoses_unterminated_strings() {
    assert_eq!(first_code("\"abc"), LEX_UNTERMINATED_STRING);
    assert_eq!(first_code("\"\"\"abc"), LEX_UNTERMINATED_BLOCK_STRING);
}
