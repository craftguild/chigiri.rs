//! Render descriptor collections as fixed-width, non-cyclic chunk tables.

use crate::writer::{GeneratedItem, RustWriter};

// The non-circular runtime DescriptorList consists of fixed widths.
const DESCRIPTORS_PER_LIST_CHUNK: usize = 64;

pub(super) fn render_descriptor_list(
    stem: &str,
    count: usize,
    rust_type: &str,
) -> Vec<GeneratedItem> {
    render_list(stem, count, rust_type, |index| {
        format!("super::{stem}_{index:04}")
    })
}

pub(super) fn render_literal_list(
    stem: &str,
    values: &[String],
    rust_type: &str,
) -> Vec<GeneratedItem> {
    render_list(stem, values.len(), rust_type, |index| values[index].clone())
}

fn render_list(
    stem: &str,
    count: usize,
    rust_type: &str,
    expression: impl Fn(usize) -> String,
) -> Vec<GeneratedItem> {
    let chunks = count.div_ceil(DESCRIPTORS_PER_LIST_CHUNK);
    let mut items = (0..chunks)
        .map(|chunk| {
            let name = chunk_name(stem, chunk);
            let start = chunk * DESCRIPTORS_PER_LIST_CHUNK;
            let end = count.min(start + DESCRIPTORS_PER_LIST_CHUNK);
            let mut writer = RustWriter::new();
            writer.line(&format!("pub(super) const {name}: &[{rust_type}] = &["));
            writer.indented(|writer| {
                for index in start..end {
                    writer.line(&format!("{},", expression(index)));
                }
            });
            writer.line("];");
            GeneratedItem::new(name, writer.finish())
        })
        .collect::<Vec<_>>();
    if chunks > 0 {
        let name = list_name(stem);
        let mut writer = RustWriter::new();
        writer.line(&format!(
            "pub(super) const {name}: ::chigiri::DescriptorList<{rust_type}> = ::chigiri::DescriptorList::new(&["
        ));
        writer.indented(|writer| {
            for chunk in 0..chunks {
                writer.line(&format!("super::{},", chunk_name(stem, chunk)));
            }
        });
        writer.line("]);");
        items.push(GeneratedItem::new(name, writer.finish()));
    }
    items
}

pub(super) fn descriptor_list_expression(stem: &str, count: usize) -> String {
    if count == 0 {
        "::chigiri::DescriptorList::empty()".to_owned()
    } else {
        format!("super::{}", list_name(stem))
    }
}

fn chunk_name(stem: &str, chunk: usize) -> String {
    format!("{stem}_CHUNK_{chunk:04}")
}

fn list_name(stem: &str) -> String {
    format!("{stem}_LIST_0000")
}
