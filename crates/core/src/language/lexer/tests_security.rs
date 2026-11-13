use super::{LEX_INVALID_CHARACTER, LEX_INVALID_UNICODE_ESCAPE, lex};
use crate::SourceId;
use crate::language::TokenKind;

#[test]
fn preserves_error_status_when_the_diagnostic_limit_is_zero() {
    let output = lex(SourceId::new(0), "%", 0);

    assert!(output.had_errors);
    assert!(!output.is_valid());
    assert!(output.diagnostics.is_empty());
    assert!(output.diagnostics.was_truncated());
    assert_eq!(output.tokens.last().unwrap().kind, TokenKind::Eof);
}

#[test]
fn lexing_stops_at_the_diagnostic_limit() {
    let output = lex(SourceId::new(0), "%%%%", 2);

    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.had_errors);
    assert_eq!(output.tokens.len(), 1);
    assert_eq!(output.tokens[0].kind, TokenKind::Eof);
    assert_eq!(output.tokens[0].span.start, 2);
}

#[test]
fn valid_input_reaches_eof_with_a_zero_diagnostic_limit() {
    let output = lex(SourceId::new(0), "query { field }", 0);

    assert!(output.is_valid());
    assert_eq!(output.tokens.last().unwrap().span.start, 15);
}

#[test]
fn long_comments_are_scanned_to_eof_with_bounded_work() {
    let source = format!("query # {}", "- I know.".repeat(100_000));
    let output = lex(SourceId::new(0), &source, 1);

    assert!(output.is_valid());
    assert_eq!(output.tokens.len(), 2);
    assert_eq!(output.tokens[0].kind, TokenKind::Name("query".to_owned()));
}

#[test]
fn huge_unicode_escape_does_not_overflow() {
    let source = format!("\"\\u{{{}}}\"", "F".repeat(100_000));
    let output = lex(SourceId::new(0), &source, 1);

    assert!(output.had_errors);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        LEX_INVALID_UNICODE_ESCAPE
    );
}

#[test]
fn advances_on_valid_byte_boundaries_for_each_invalid_multibyte_character() {
    let output = lex(SourceId::new(0), "λ€", 2);
    let spans: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.span.unwrap())
        .collect();

    assert_eq!(spans[0].start, 0);
    assert_eq!(spans[0].end, 2);
    assert_eq!(spans[1].start, 2);
    assert_eq!(spans[1].end, 5);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == LEX_INVALID_CHARACTER)
    );
}

#[test]
fn terminates_for_short_combinations_of_boundary_characters() {
    let characters = [
        '\0', '\u{0007}', '\t', '\r', '\n', ' ', '#', ',', '.', '-', '0', 'e', '_', 'A', '"', '\\',
        '{', '}', '\u{FEFF}', 'λ', '💩',
    ];
    for first in characters {
        for second in characters {
            for third in characters {
                let source: String = [first, second, third].into_iter().collect();
                let output = lex(SourceId::new(0), &source, 2);
                assert!(matches!(
                    output.tokens.last().map(|token| &token.kind),
                    Some(TokenKind::Eof)
                ));
            }
        }
    }
}
