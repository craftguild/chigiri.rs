//! Converts closed TOML tables and bounded scalars to positioned diagnostics.

use crate::{CodegenDiagnostic, CodegenError, DiagnosticLabel, DiagnosticReport};
use toml_edit::{Item, Table};

pub(super) struct Context<'a> {
    pub label: &'a str,
    pub source: &'a str,
}

impl Context<'_> {
    pub fn error(&self, item: Option<&Item>, message: &str) -> CodegenError {
        let offset = item.and_then(Item::span).map_or(0, |span| span.start);
        self.error_at(offset, "SF4011", message)
    }

    pub fn error_at(&self, offset: usize, code: &'static str, message: &str) -> CodegenError {
        CodegenError::from_report(DiagnosticReport::new(
            vec![CodegenDiagnostic::error(code, message).with_primary(self.label(offset))],
            false,
        ))
    }

    pub fn label(&self, offset: usize) -> DiagnosticLabel {
        let prefix = &self.source[..offset.min(self.source.len())];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        // Do not copy source excerpts or raw parser input because a manifest may
        // contain credentials entered by mistake.
        DiagnosticLabel::new(self.label, offset..offset, line, column, "manifest entry")
    }

    pub fn closed(&self, table: &Table, keys: &[&str]) -> Result<(), CodegenError> {
        for (key, item) in table.iter() {
            if !keys.contains(&key) {
                return Err(self.error(Some(item), "The manifest has an unknown key."));
            }
        }
        Ok(())
    }

    pub fn table<'a>(
        &self,
        table: &'a Table,
        key: &str,
        keys: &[&str],
    ) -> Result<&'a Table, CodegenError> {
        let item = table.get(key);
        let value = item.and_then(Item::as_table).ok_or_else(|| {
            self.error(item, "A required table is missing or has the wrong type.")
        })?;
        if value.is_dotted() || value.is_implicit() {
            return Err(self.error(item, "Specify the manifest as an explicit table."));
        }
        self.closed(value, keys)?;
        Ok(value)
    }

    pub fn string(&self, table: &Table, key: &str) -> Result<String, CodegenError> {
        let item = table.get(key);
        item.and_then(Item::as_str)
            .filter(|value| {
                !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
            })
            .map(str::to_owned)
            .ok_or_else(|| self.error(item, "The value must be a nonempty bounded string."))
    }

    pub fn boolean(&self, table: &Table, key: &str, default: bool) -> Result<bool, CodegenError> {
        match table.get(key) {
            None => Ok(default),
            Some(item) => item
                .as_bool()
                .ok_or_else(|| self.error(Some(item), "Expected a boolean.")),
        }
    }

    pub fn strings(
        &self,
        table: &Table,
        key: &str,
        duplicate_code: &'static str,
    ) -> Result<Vec<String>, CodegenError> {
        let Some(item) = table.get(key) else {
            return Ok(Vec::new());
        };
        let array = item
            .as_array()
            .filter(|array| array.len() <= 1024)
            .ok_or_else(|| self.error(Some(item), "Expected an array of at most 1024 strings."))?;
        let mut seen = std::collections::BTreeSet::new();
        let mut values = Vec::new();
        for value in array {
            let name = value
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .ok_or_else(|| {
                    self.error(Some(item), "The array contains a non-string element.")
                })?;
            if !seen.insert(name) {
                return Err(self.error_at(
                    value.span().map_or(0, |span| span.start),
                    duplicate_code,
                    "The array contains duplicate elements.",
                ));
            }
            values.push(name.to_owned());
        }
        Ok(values)
    }

    pub fn require(
        &self,
        table: &Table,
        key: &str,
        valid: bool,
        message: &str,
    ) -> Result<(), CodegenError> {
        if valid {
            Ok(())
        } else {
            Err(self.error(table.get(key), message))
        }
    }
}
