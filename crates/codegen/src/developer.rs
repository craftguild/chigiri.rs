//! Builds high-confidence doctor guidance from the same schema model.

use chigiri_core::ExecutionLimits;
use chigiri_core::schema::TypeKind;

use crate::config::ResolvedBindings;
use crate::error::CodegenError;
use crate::naming::canonical_type_ref;
use crate::render::model::{RenderModel, field_name};

/// GraphQL-to-Rust API usage guidance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperField {
    coordinate: String,
    field_name: String,
    graphql_output: String,
    rust_output: String,
}

impl DeveloperField {
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    pub fn graphql_output(&self) -> &str {
        &self.graphql_output
    }

    pub fn rust_output(&self) -> &str {
        &self.rust_output
    }
}

/// Source-defined object and its complete resolver contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperObject {
    graphql_name: String,
    resolver_trait: String,
    assembly_method: String,
    fields: Vec<DeveloperField>,
}

impl DeveloperObject {
    pub fn graphql_name(&self) -> &str {
        &self.graphql_name
    }

    pub fn resolver_trait(&self) -> &str {
        &self.resolver_trait
    }

    pub fn assembly_method(&self) -> &str {
        &self.assembly_method
    }

    pub fn fields(&self) -> &[DeveloperField] {
        &self.fields
    }
}

/// Developer guidance for doctor from one compiled schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperSchema {
    objects: Vec<DeveloperObject>,
}

impl DeveloperSchema {
    pub fn objects(&self) -> &[DeveloperObject] {
        &self.objects
    }

    pub fn object(&self, graphql_name: &str) -> Option<&DeveloperObject> {
        self.objects
            .iter()
            .find(|object| object.graphql_name == graphql_name)
    }
}

/// Builds doctor hints from schema bindings and the loaded manifest.
pub fn inspect_contract_schema(
    manifest: &crate::ResolvedManifest,
) -> Result<DeveloperSchema, CodegenError> {
    let compiled = manifest.compile()?;
    let bindings = ResolvedBindings::from_manifest(manifest, &compiled.schema)?;
    inspect(&compiled, &bindings)
}

fn inspect(
    compiled: &crate::compile::CompiledSchema,
    bindings: &ResolvedBindings,
) -> Result<DeveloperSchema, CodegenError> {
    let model = RenderModel::build_with_bindings(&compiled.schema, &compiled.source_map, bindings)?;
    let mut objects = Vec::new();
    for object in model
        .user_types()
        .filter(|object| object.kind() == TypeKind::Object)
    {
        let graphql_name = object.name().to_owned();
        let rust_name = model.type_name(object.name()).to_owned();
        let mut fields = Vec::new();
        for field in object.fields() {
            let member = field_name(&field.name.value);
            fields.push(DeveloperField {
                coordinate: format!("{}.{name}", object.name(), name = field.name.value),
                field_name: member,
                graphql_output: canonical_type_ref(
                    &field.ty,
                    ExecutionLimits::default().max_parser_nesting,
                )
                .map_err(|diagnostic| {
                    CodegenError::new(diagnostic.code.as_str(), diagnostic.message)
                })?,
                rust_output: model.output_type(&field.ty)?,
            });
        }
        objects.push(DeveloperObject {
            graphql_name,
            resolver_trait: format!("{rust_name}Resolver"),
            assembly_method: model.resolver_method_name(object.name()).to_owned(),
            fields,
        });
    }
    Ok(DeveloperSchema { objects })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::inspect_contract_schema;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn resolver_method_types_follow_source_return_order() {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("chigiri-developer-{}-{serial}", std::process::id()));
        fs::create_dir_all(root.join("graphql")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let manifest_path = root.join("chigiri.toml");
        fs::write(
            &manifest_path,
            format!(
                "{}\n[runtime]\nsource=\"path\"\nroot=\".\"\n",
                crate::MANIFEST_TEMPLATE
            ),
        )
        .unwrap();
        fs::write(
            root.join("graphql/schema.graphql"),
            "type Query { book: Book }\ntype Book { id: ID!, reviews(first: Int): [String!]! }\n",
        )
        .unwrap();
        let manifest = crate::ResolvedManifest::load(&manifest_path).unwrap();
        let schema = inspect_contract_schema(&manifest).unwrap();
        let query = schema.object("Query").unwrap();
        let book = schema.object("Book").unwrap();
        assert_eq!(query.assembly_method(), "with_query");
        assert_eq!(book.fields()[1].graphql_output(), "[String!]!");
        fs::remove_dir_all(root).unwrap();
    }
}
