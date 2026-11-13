use super::{ParseOutput, parse_document, parse_executable_document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

#[test]
fn short_input_corpus_respects_input_and_diagnostic_limits_without_panicking() {
    let mut frontier = vec![String::new()];
    let mut corpus = Vec::new();
    for _ in 0..=4 {
        corpus.extend(frontier.iter().cloned());
        frontier = frontier
            .iter()
            .flat_map(|prefix| {
                ['{', '}', '[', ']', '$', 'a', '"'].map(move |character| {
                    let mut value = prefix.clone();
                    value.push(character);
                    value
                })
            })
            .collect();
    }
    corpus.extend(["{ f(a: \"Name\") }".to_owned(), "\u{FEFF}query".to_owned()]);

    for source in corpus {
        for max_diagnostics in [0, 1, 3] {
            let mut limits = ExecutionLimits::unbounded();
            limits.max_diagnostics = max_diagnostics;
            check_output(
                &source,
                max_diagnostics,
                parse_document(SourceId::new(9), &source, &limits),
            );
            check_output(
                &source,
                max_diagnostics,
                parse_executable_document(SourceId::new(9), &source, &limits),
            );
            check_output(
                &source,
                max_diagnostics,
                parse_type_system_document(SourceId::new(9), &source, &limits),
            );
        }
    }
}

fn check_output(source: &str, max_diagnostics: usize, output: ParseOutput) {
    assert!(output.diagnostics.len() <= max_diagnostics);
    if !output.diagnostics.is_empty() {
        assert!(output.document.is_none());
    }
    for diagnostic in output.diagnostics.iter() {
        let span = diagnostic
            .span
            .expect("Parser diagnostic has a source span.");
        assert_eq!(span.source_id, SourceId::new(9));
        assert!(span.start <= span.end && span.end <= source.len());
        assert!(source.is_char_boundary(span.start));
        assert!(source.is_char_boundary(span.end));
    }
}
