//! Assigns generated items to file paths while checking resource limits and path collisions.

use std::collections::BTreeMap;

use chigiri_core::DiagnosticCode;

use super::{GENERATED_HEADER, GeneratedFile, GeneratedItem, WriterError};

pub(crate) fn assemble_items(
    items: impl IntoIterator<Item = GeneratedItem>,
) -> Result<Vec<GeneratedFile>, WriterError> {
    let mut files = BTreeMap::<String, GeneratedFile>::new();
    let mut case_paths = BTreeMap::new();
    let mut directories = BTreeMap::new();
    let mut total = 0usize;
    for item in items {
        let (name, contents, public) = item.into_parts();
        let parts = crate::filesystem::relative_components(&name).map_err(|_| invalid())?;
        if !name.ends_with(".rs") {
            return Err(invalid());
        }
        let folded = name.to_ascii_lowercase();
        if case_paths.get(&folded).is_some_and(|path| path != &name) {
            return Err(collision());
        }
        case_paths.insert(folded, name.clone());
        for length in 1..parts.len() {
            let directory = parts[..length].join("/");
            let folded = directory.to_ascii_lowercase();
            if directories
                .get(&folded)
                .is_some_and(|path| path != &directory)
            {
                return Err(collision());
            }
            directories.insert(folded, directory);
        }
        let extra = if files.contains_key(&name) {
            1
        } else {
            GENERATED_HEADER.len()
        };
        total = total
            .checked_add(contents.len())
            .and_then(|value| value.checked_add(extra))
            .ok_or_else(invalid)?;
        if total > 64 * 1024 * 1024 || case_paths.len() > 65_536 {
            return Err(invalid());
        }
        match files.get_mut(&name) {
            Some(file) => {
                file.contents.push('\n');
                file.contents.push_str(&contents);
                file.exports_public_items |= public;
            }
            None => {
                let mut source = String::with_capacity(GENERATED_HEADER.len() + contents.len());
                source.push_str(GENERATED_HEADER);
                source.push_str(&contents);
                files.insert(
                    name.clone(),
                    GeneratedFile {
                        name,
                        contents: source,
                        exports_public_items: public,
                    },
                );
            }
        }
    }
    if directories.keys().any(|path| case_paths.contains_key(path)) {
        return Err(collision());
    }
    Ok(files.into_values().collect())
}

fn invalid() -> WriterError {
    WriterError {
        code: DiagnosticCode::new("SF4010"),
        message: "The generated-file path or byte/file resource limit is invalid.".to_owned(),
    }
}

fn collision() -> WriterError {
    WriterError {
        code: DiagnosticCode::new("SF4006"),
        message: "The case or directory path of the generated file is colliding.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::assemble_items;
    use crate::writer::GeneratedItem;

    #[test]
    fn semantic_paths_reject_traversal_case_and_file_directory_collisions() {
        for path in [
            "../escape.rs",
            "/escape.rs",
            "types/../escape.rs",
            "types/a.txt",
        ] {
            assert!(assemble_items([GeneratedItem::new(path, "")]).is_err());
        }
        for names in [
            ["types/Book.rs", "types/book.rs"],
            ["types/Owner/first.rs", "types/owner/second.rs"],
            ["types/book.rs", "types/book.rs/nested.rs"],
        ] {
            let error = assemble_items(names.map(|path| GeneratedItem::new(path, ""))).unwrap_err();
            assert_eq!(error.code.as_str(), "SF4006");
        }
    }

    #[test]
    fn sg_layout_06_rejects_total_byte_overflow_before_adding_a_file() {
        let item = GeneratedItem::new("types/huge.rs", " ".repeat(64 * 1024 * 1024));
        let error = assemble_items([item]).unwrap_err();
        assert_eq!(error.code.as_str(), "SF4010");
    }
}
