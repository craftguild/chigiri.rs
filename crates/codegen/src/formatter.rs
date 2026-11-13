//! Format generated Rust with the fixed Rust 1.95.0 toolchain.

use std::process::Command;
use std::time::Duration;

use chigiri_host::filesystem::TemporaryFile;

use crate::error::CodegenError;
use crate::writer::GeneratedItem;
use crate::{ProcessLimits, run_bounded_process};

const RUSTFMT_CONFIG: &str = include_str!("../rustfmt.generated.toml");
const MAX_FORMAT_BATCH_ITEMS: usize = 512;
const MAX_FORMAT_BATCH_BYTES: usize = 2 * 1024 * 1024;

pub(crate) fn format_items(items: Vec<GeneratedItem>) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut output = Vec::with_capacity(items.len());
    let mut batch = Vec::new();
    let mut batch_bytes = 0usize;
    for item in items {
        let part = item.into_parts();
        let part_bytes = part.1.len().saturating_add(128);
        if part_bytes > MAX_FORMAT_BATCH_BYTES {
            return Err(CodegenError::new(
                "SF4010",
                format!(
                    "The generated item `{}` exceeds the rustfmt batch byte limit.",
                    part.0
                ),
            ));
        }
        if !batch.is_empty()
            && (batch.len() >= MAX_FORMAT_BATCH_ITEMS
                || batch_bytes.saturating_add(part_bytes) > MAX_FORMAT_BATCH_BYTES)
        {
            output.extend(format_batch(std::mem::take(&mut batch))?);
            batch_bytes = 0;
        }
        batch_bytes = batch_bytes.saturating_add(part_bytes);
        batch.push(part);
    }
    if !batch.is_empty() {
        output.extend(format_batch(batch)?);
    }
    Ok(output)
}

fn format_batch(parts: Vec<(String, String, bool)>) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut source = String::new();
    for (index, (_, contents, _)) in parts.iter().enumerate() {
        source.push_str(&start_marker(index));
        source.push('\n');
        source.push_str(contents);
        if !contents.ends_with('\n') {
            source.push('\n');
        }
        source.push_str(&end_marker(index));
        source.push('\n');
    }
    let formatted = format_source("generated item batch", &source)?;
    split_batch(parts, &formatted)
}

pub(crate) fn format_source(label: &str, contents: &str) -> Result<String, CodegenError> {
    let config = RustfmtConfig::create()?;
    let mut command = Command::new("rustup");
    command.args([
        "run",
        "1.95.0",
        "rustfmt",
        "--emit",
        "stdout",
        "--config-path",
    ]);
    config.push_argument(&mut command)?;
    let output = run_bounded_process(
        &mut command,
        contents.as_bytes(),
        ProcessLimits { timeout: Duration::from_secs(300), bytes: 64 * 1024 * 1024 },
    )
        .map_err(|error| {
            if error == crate::ProcessFailure::Cancelled {
                return crate::cancellation::error();
            }
            CodegenError::new(
                "SF4004",
                "Unable to run rustfmt 1.95.0. Install it with `rustup component add rustfmt --toolchain 1.95.0` and verify that rustup can execute it.",
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError::new(
            "SF4004",
            format!(
                "rustfmt 1.95.0 failed while formatting generated item `{label}`: {}",
                stderr.trim()
            ),
        ));
    }
    let formatted = String::from_utf8(output.stdout).map_err(|error| {
        CodegenError::new(
            "SF4004",
            format!("rustfmt 1.95.0 produced non-UTF-8 output: {error}"),
        )
    })?;
    Ok(formatted)
}

struct RustfmtConfig {
    file: TemporaryFile,
}

impl RustfmtConfig {
    fn create() -> Result<Self, CodegenError> {
        TemporaryFile::create("rustfmt.toml", RUSTFMT_CONFIG.as_bytes())
            .map(|file| Self { file })
            .map_err(|_| {
                CodegenError::new(
                    "SF4004",
                    "Unable to create the private rustfmt 1.95.0 configuration.",
                )
            })
    }

    fn push_argument<'a>(&self, command: &'a mut Command) -> Result<&'a mut Command, CodegenError> {
        self.file.push_argument(command).map_err(|_| {
            CodegenError::new(
                "SF4004",
                "The private rustfmt 1.95.0 configuration changed before use.",
            )
        })
    }
}

fn split_batch(
    parts: Vec<(String, String, bool)>,
    formatted: &str,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut output = Vec::with_capacity(parts.len());
    let mut lines = formatted.split_inclusive('\n');
    for (index, (label, _, exports_public_item)) in parts.into_iter().enumerate() {
        let start = start_marker(index);
        let end = end_marker(index);
        let start_line = lines
            .next()
            .ok_or_else(|| boundary_error(&label, "start marker"))?;
        if logical_line(start_line) != start {
            return Err(boundary_error(&label, "start marker"));
        }
        let mut contents = String::new();
        loop {
            let line = lines
                .next()
                .ok_or_else(|| boundary_error(&label, "end marker"))?;
            if logical_line(line) == end {
                break;
            }
            contents.push_str(line);
        }
        let item = GeneratedItem::new(label, contents.trim_end_matches('\n'));
        output.push(if exports_public_item {
            item.exported()
        } else {
            item
        });
    }
    Ok(output)
}

fn logical_line(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn boundary_error(label: &str, boundary: &str) -> CodegenError {
    CodegenError::new(
        "SF4004",
        format!("rustfmt output is missing the {boundary} for generated item `{label}`"),
    )
}

fn start_marker(index: usize) -> String {
    format!("// __CHIGIRI_ITEM_{index:08}_START__")
}

fn end_marker(index: usize) -> String {
    format!("// __CHIGIRI_ITEM_{index:08}_END__")
}

#[cfg(test)]
mod tests {
    use crate::writer::GeneratedItem;

    use super::format_items;

    #[test]
    fn restore_multiple_items_from_one_rustfmt_batch() {
        let items = vec![
            GeneratedItem::new("first", "pub fn first()->usize{1}").exported(),
            GeneratedItem::new("second", "pub fn second()->usize{2}"),
        ];

        let formatted = format_items(items).unwrap();

        assert_eq!(formatted.len(), 2);
        let (_, first, first_exported) = formatted[0].clone().into_parts();
        let (_, second, second_exported) = formatted[1].clone().into_parts();
        assert_eq!(first, "pub fn first() -> usize {\n    1\n}\n");
        assert_eq!(second, "pub fn second() -> usize {\n    2\n}\n");
        assert!(first_exported);
        assert!(!second_exported);
    }

    #[test]
    fn marker_text_inside_a_string_is_not_treated_as_a_boundary() {
        let source = "pub const VALUE: &str = \"// __CHIGIRI_ITEM_00000000_END__\";";

        let formatted = format_items(vec![GeneratedItem::new("marker", source)]).unwrap();
        let (_, contents, _) = formatted.into_iter().next().unwrap().into_parts();

        assert!(contents.contains("__CHIGIRI_ITEM_00000000_END__"));
        assert!(contents.contains("pub const VALUE"));
    }

    #[test]
    fn batches_preserve_every_numbered_item() {
        let items = (0..=super::MAX_FORMAT_BATCH_ITEMS)
            .map(|index| {
                GeneratedItem::new(
                    format!("item-{index}"),
                    format!("const V{index}:usize={index};"),
                )
            })
            .collect();

        let formatted = format_items(items).unwrap();

        assert_eq!(formatted.len(), super::MAX_FORMAT_BATCH_ITEMS + 1);
        assert!(
            formatted
                .last()
                .unwrap()
                .clone()
                .into_parts()
                .1
                .contains("V512")
        );
    }
}
