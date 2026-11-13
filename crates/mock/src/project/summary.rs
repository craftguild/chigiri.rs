//! Record semantic SDL elements and digests for change reporting without copying source text.

use std::collections::BTreeMap;

use chigiri::core::Span;
use serde::{Deserialize, Serialize};

use super::ProjectError;
use super::digest::Digest;
use crate::ir::{FieldId, MockIr};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Summary {
    pub query: String,
    pub mutation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,
    #[serde(deserialize_with = "super::metadata::facts")]
    pub entries: BTreeMap<String, Digest>,
}

pub(super) fn build(ir: &MockIr) -> Result<Summary, ProjectError> {
    let graph = ir.graph();
    let mut summary = Summary {
        query: graph.types()[graph.query().0].name.to_string(),
        mutation: graph
            .mutation()
            .map(|id| graph.types()[id.0].name.to_string()),
        subscription: graph
            .subscription()
            .map(|id| graph.types()[id.0].name.to_string()),
        entries: BTreeMap::new(),
    };
    let mut bytes = 0usize;
    let mut add = |key: String, digest: Digest| -> Result<(), ProjectError> {
        bytes = bytes.checked_add(key.len() + 64).ok_or_else(limit)?;
        if bytes > super::metadata::MAX_BYTES {
            return Err(limit());
        }
        summary.entries.insert(key, digest);
        Ok(())
    };
    for (id, node) in graph.types().iter().enumerate() {
        let semantic = &graph.source().semantic().types()[id];
        let mut parts = vec![format!("{:?}", node.kind)];
        parts.extend(node.enum_values.iter().map(ToString::to_string));
        parts.extend(
            node.interfaces
                .iter()
                .map(|id| graph.types()[id.0].name.to_string()),
        );
        parts.extend(
            node.possible_types
                .iter()
                .map(|id| graph.types()[id.0].name.to_string()),
        );
        for directive in semantic.directives() {
            parts.push(text(ir, directive.span)?.into());
        }
        add(
            format!("type/{}", node.name),
            Digest::bytes(parts.join("\n").as_bytes()),
        )?;
        for input in semantic.input_fields() {
            add(
                format!("input/{}/{}", node.name, input.name.value),
                stamp(ir, input.span)?,
            )?;
        }
        if let Some(model) = ir.model_for(crate::ir::TypeId(id)) {
            // Track model addition, removal, and key changes independently of field changes.
            add(
                format!("model/{}", node.name),
                Digest::bytes(ir.models()[model.0].key.map_or(b"none".as_slice(), |key| {
                    graph.fields()[key.0].name.as_bytes()
                })),
            )?;
        }
    }
    for (id, field) in graph.fields().iter().enumerate() {
        let coordinate = format!("{}.{}", graph.types()[field.parent.0].name, field.name);
        let definition = graph.field_source(FieldId(id)).ok_or_else(invalid)?;
        add(format!("field/{coordinate}"), stamp(ir, definition.span)?)?;
        for argument in &definition.arguments {
            add(
                format!("argument/{coordinate}/{}", argument.name.value),
                stamp(ir, argument.span)?,
            )?;
        }
        for directive in &definition.directives {
            let kind = match directive.name.value.as_str() {
                "constraint" => "constraint",
                "relation" => "relation",
                _ => continue,
            };
            add(format!("{kind}/{coordinate}"), stamp(ir, directive.span)?)?;
        }
    }
    Ok(summary)
}

fn text(ir: &MockIr, span: Span) -> Result<&str, ProjectError> {
    ir.graph()
        .source()
        .sources()
        .get(span.source_id.index())
        .and_then(|source| source.text().get(span.start..span.end))
        .ok_or_else(invalid)
}
fn stamp(ir: &MockIr, span: Span) -> Result<Digest, ProjectError> {
    Ok(Digest::bytes(text(ir, span)?.as_bytes()))
}
fn invalid() -> ProjectError {
    ProjectError::new("schema", "invalid snapshot source span")
}
fn limit() -> ProjectError {
    ProjectError::new("limit", "schema snapshot byte limit")
}
