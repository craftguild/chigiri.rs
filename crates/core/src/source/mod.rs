//! Maps source text and byte spans to line and column locations.

use crate::Location;
use std::string::FromUtf8Error;

/// Stable index identifying a source in [`SourceMap`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(usize);

impl SourceId {
    /// Create a source ID from its internal index.
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Return the internal [`SourceMap`] index.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Half-open byte range within one source.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Span {
    /// Source containing this span.
    pub source_id: SourceId,
    /// Start byte offset in the UTF-8 text.
    pub start: usize,
    /// End byte offset in the UTF-8 text.
    pub end: usize,
}

impl Span {
    /// Create a half-open byte range.
    pub const fn new(source_id: SourceId, start: usize, end: usize) -> Self {
        Self {
            source_id,
            start,
            end,
        }
    }

    /// Return the byte length.
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span is empty.
    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

/// Diagnostic name, UTF-8 text, and precomputed line starts.
#[derive(Clone, Debug)]
pub struct Source {
    name: String,
    text: String,
    line_starts: Vec<usize>,
}

impl Source {
    /// Create a source and compute its line-start byte offsets once.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = collect_line_starts(&text);
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// Return the source name shown in diagnostics.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the complete source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Return precomputed line-start byte offsets.
    pub fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    /// Convert a byte offset to a one-based Unicode scalar line and column.
    ///
    /// Reject offsets past the text or inside a UTF-8 code point. GraphQL source
    /// positions count each Unicode scalar as one column.
    pub fn location(&self, byte_offset: usize) -> Option<Location> {
        if byte_offset > self.text.len() || !self.text.is_char_boundary(byte_offset) {
            return None;
        }
        let line_index = match self.line_starts.binary_search(&byte_offset) {
            Ok(index) => index,
            Err(index) => index.checked_sub(1)?,
        };
        let line_start = self.line_starts[line_index];
        let column = self.text[line_start..byte_offset].chars().count() + 1;
        Some(Location::new(line_index + 1, column))
    }

    /// Return text for a one-based line number without its line ending.
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let index = line.checked_sub(1)?;
        let start = *self.line_starts.get(index)?;
        let end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        Some(self.text[start..end].trim_end_matches(['\r', '\n']))
    }
}

/// Map containing multiple sources and their stable IDs.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    sources: Vec<Source>,
}

impl SourceMap {
    /// Create an empty source map.
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Add a UTF-8 string source and return its assigned ID.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let id = SourceId::new(self.sources.len());
        self.sources.push(Source::new(name, text));
        id
    }

    /// Validate byte input at the UTF-8 ingestion boundary and add it.
    pub fn add_bytes(
        &mut self,
        name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<SourceId, FromUtf8Error> {
        let text = String::from_utf8(bytes)?;
        Ok(self.add(name, text))
    }

    /// Return the source for an ID.
    pub fn get(&self, id: SourceId) -> Option<&Source> {
        self.sources.get(id.index())
    }

    /// Return the number of registered sources.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Whether no sources are registered.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Iterate over sources with IDs in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (SourceId, &Source)> {
        self.sources
            .iter()
            .enumerate()
            .map(|(index, source)| (SourceId::new(index), source))
    }

    /// Convert the start of a span into a source location.
    pub fn location(&self, span: Span) -> Option<Location> {
        self.get(span.source_id)?.location(span.start)
    }

    /// Return the text selected by a span whose offsets are valid UTF-8 boundaries.
    pub fn slice(&self, span: Span) -> Option<&str> {
        if span.start > span.end {
            return None;
        }
        self.get(span.source_id)?.text().get(span.start..span.end)
    }
}

fn collect_line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => index += 2,
            b'\r' | b'\n' => index += 1,
            _ => {
                index += 1;
                continue;
            }
        }
        starts.push(index);
    }
    starts
}

#[cfg(test)]
mod tests;
