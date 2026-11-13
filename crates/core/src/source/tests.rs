use super::{SourceMap, Span};
use crate::Location;

#[test]
fn unicode_uses_character_columns_and_byte_spans() {
    let mut sources = SourceMap::new();
    let id = sources.add("unicode.graphql", "αβ\r\n\tfield\n€");
    let source = sources.get(id).unwrap();

    assert_eq!(source.line_starts(), &[0, 6, 13]);
    assert_eq!(source.location(2), Some(Location::new(1, 2)));
    assert_eq!(source.location(7), Some(Location::new(2, 2)));
    assert_eq!(source.location(13), Some(Location::new(3, 1)));
    assert_eq!(source.location(1), None);

    let span = Span::new(id, 7, 12);
    assert_eq!(span.len(), 5);
    assert_eq!(sources.slice(span), Some("field"));
}

#[test]
fn counts_carriage_return_and_line_feed_as_line_terminators() {
    let mut sources = SourceMap::new();
    let id = sources.add("lines.graphql", "a\r\nb\rc\nd");
    let source = sources.get(id).unwrap();

    assert_eq!(source.line_starts(), &[0, 3, 5, 7]);
    assert_eq!(source.line_text(1), Some("a"));
    assert_eq!(source.line_text(2), Some("b"));
    assert_eq!(source.line_text(3), Some("c"));
    assert_eq!(source.line_text(4), Some("d"));
}

#[test]
fn rejects_invalid_utf8_before_adding_a_source() {
    let mut sources = SourceMap::new();

    assert!(
        sources
            .add_bytes("invalid.graphql", vec![0x66, 0x80])
            .is_err()
    );
    assert!(sources.is_empty());
}

#[test]
fn validates_span_source_ids_and_utf8_boundaries() {
    let mut sources = SourceMap::new();
    let id = sources.add("source.graphql", "€");

    assert_eq!(sources.slice(Span::new(id, 0, 3)), Some("€"));
    assert_eq!(sources.slice(Span::new(id, 1, 3)), None);
    assert_eq!(sources.slice(Span::new(id, 3, 2)), None);
}

#[test]
fn eof_locations_and_trailing_empty_lines_are_reported_consistently() {
    let mut sources = SourceMap::new();
    let empty = sources.add("empty.graphql", "");
    let trailing = sources.add("trailing.graphql", "a\n");

    assert_eq!(
        sources.get(empty).unwrap().location(0),
        Some(Location::new(1, 1))
    );
    assert_eq!(
        sources.get(trailing).unwrap().location(2),
        Some(Location::new(2, 1))
    );
    assert_eq!(sources.get(trailing).unwrap().line_text(2), Some(""));
}
