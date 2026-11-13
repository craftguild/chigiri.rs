use super::Diagnostic;
use crate::{Source, SourceMap, Span};
use std::fmt::Write;

pub(super) fn render_diagnostic(diagnostic: &Diagnostic, sources: &SourceMap) -> String {
    let mut output = format!("error[{}]: {}", diagnostic.code, diagnostic.message);
    let Some(span) = diagnostic.span else {
        return output;
    };
    let Some(source) = sources.get(span.source_id) else {
        output.push_str("\n --> <unknown source>");
        return output;
    };
    if source.text().get(span.start..span.end).is_none() {
        let _ = write!(output, "\n --> {}:<invalid span>", source.name());
        return output;
    }
    let Some(location) = source.location(span.start) else {
        let _ = write!(output, "\n --> {}:<invalid span>", source.name());
        return output;
    };

    let _ = write!(
        output,
        "\n --> {}:{}:{}",
        source.name(),
        location.line,
        location.column
    );
    append_excerpt(&mut output, source, span, location.line);
    output
}

fn append_excerpt(output: &mut String, source: &Source, span: Span, line: usize) {
    let Some(line_text) = source.line_text(line) else {
        return;
    };
    let width = line.to_string().len();
    let caret_len = caret_length(source, span, line_text).max(1);
    let _ = write!(
        output,
        "\n{:width$} |\n{line} | {line_text}\n{:width$} | ",
        "", ""
    );
    append_caret_prefix(output, source, span, line);
    output.push('^');
    output.extend(std::iter::repeat_n('~', caret_len.saturating_sub(1)));
}

fn append_caret_prefix(output: &mut String, source: &Source, span: Span, line: usize) {
    let line_start = source.line_starts()[line - 1];
    let Some(prefix) = source.text().get(line_start..span.start) else {
        return;
    };
    for character in prefix.chars() {
        // Preserve source tabs so the displayed caret remains aligned.
        output.push(if character == '\t' { '\t' } else { ' ' });
    }
}

fn caret_length(source: &Source, span: Span, line_text: &str) -> usize {
    let Some(location) = source.location(span.start) else {
        return 1;
    };
    let line_start = source.line_starts()[location.line - 1];
    let line_end = line_start + line_text.len();
    let end = span.end.min(line_end);
    if end <= span.start || !source.text().is_char_boundary(end) {
        return 1;
    }
    source.text()[span.start..end].chars().count()
}
