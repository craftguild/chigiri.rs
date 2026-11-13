//! Rebuild schema source and vocabulary without accessing the original project files.

use chigiri::core::{SchemaDigest, Source};
use chigiri::experimental::OwnedSchema;

use super::{SchemaError, VOCABULARY_SOURCE, VOCABULARY_VERSION, parse_sources};

/// Validate embedded source order, vocabulary version, and digest through one parse path.
///
/// Digest equality detects snapshot drift; it is not complete semantic validation.
/// This function performs no filesystem or network access. The IR validates Mock semantics.
pub fn parse_snapshot(
    mut sources: Vec<Source>,
    digest: SchemaDigest,
    version: &str,
) -> Result<OwnedSchema, SchemaError> {
    if version != VOCABULARY_VERSION {
        return Err(SchemaError::new("unsupported vocabulary version", None));
    }
    if !(2..=65).contains(&sources.len())
        || sources
            .iter()
            .try_fold(0usize, |bytes, source| {
                bytes.checked_add(source.text().len())
            })
            .is_none_or(|bytes| bytes > 256 * 1024)
    {
        return Err(SchemaError::new("snapshot source budget exceeded", None));
    }
    let appendix = sources
        .pop()
        .ok_or_else(|| SchemaError::new("missing vocabulary source", None))?;
    if appendix.name() != VOCABULARY_SOURCE {
        return Err(SchemaError::new("missing vocabulary source", None));
    }
    let schema = parse_sources(sources)?;
    if schema.digest() != digest
        || schema
            .sources()
            .last()
            .is_none_or(|expected| expected.text() != appendix.text())
    {
        return Err(SchemaError::new("schema snapshot drift", None));
    }
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsm_19_embedded_sources_rebuild_without_original_files_and_detect_drift() {
        let original = parse_sources(vec![
            Source::new("models.graphql", "type Item{id:ID!}"),
            Source::new("query.graphql", "type Query{items:[Item]}"),
        ])
        .unwrap();
        let rebuilt = parse_snapshot(
            original.sources().to_vec(),
            original.digest(),
            VOCABULARY_VERSION,
        )
        .unwrap();
        assert_eq!(rebuilt.digest(), original.digest());
        assert!(!rebuilt.same_instance(&original));
        assert!(parse_snapshot(original.sources().to_vec(), original.digest(), "v0").is_err());
        assert!(
            parse_snapshot(
                original.sources().to_vec(),
                original.digest(),
                "chigiri-mock-directives/v2"
            )
            .is_err()
        );
        let mut old_source = original.sources().to_vec();
        old_source[2] = Source::new("chigiri-mock/v2.graphql", old_source[2].text());
        assert_eq!(
            parse_snapshot(old_source, original.digest(), VOCABULARY_VERSION)
                .err()
                .unwrap()
                .code,
            "missing vocabulary source"
        );
        let mut reordered = original.sources().to_vec();
        reordered.swap(0, 1);
        assert_eq!(
            parse_snapshot(reordered, original.digest(), VOCABULARY_VERSION)
                .err()
                .unwrap()
                .code,
            "schema snapshot drift"
        );
        let mut changed = original.sources().to_vec();
        changed[1] = Source::new("query.graphql", "type Query{item:Item}");
        assert_eq!(
            parse_snapshot(changed, original.digest(), VOCABULARY_VERSION)
                .err()
                .unwrap()
                .code,
            "schema snapshot drift"
        );
        let mut library = original.sources().to_vec();
        library[2] = Source::new(
            VOCABULARY_SOURCE,
            format!("{}\n# modified", library[2].text()),
        );
        let changed_digest = OwnedSchema::parse(library.clone()).unwrap().digest();
        assert_eq!(
            parse_snapshot(library, changed_digest, VOCABULARY_VERSION)
                .err()
                .unwrap()
                .code,
            "schema snapshot drift"
        );
        assert!(
            parse_snapshot(
                original.sources()[..2].to_vec(),
                original.digest(),
                VOCABULARY_VERSION
            )
            .is_err()
        );
    }
}
