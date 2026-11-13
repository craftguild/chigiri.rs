//! Verifies the large synthetic consumer from ADR-0007 with a manifest and an
//! independent SDK.

use std::fs;

use chigiri_core::ExecutionLimits;
use chigiri_core::language::ast::TypeSystemDefinition;

#[path = "../../codegen/tests/support/cargo.rs"]
mod cargo;
#[path = "../../codegen/tests/vnext_consumer/fixture.rs"]
mod fixture;

use fixture::{Fixture, fixture_root, generated_files};

#[test]
fn large_fixture_preserves_the_schema_size_required_by_the_adr() {
    let source = fs::read_to_string(fixture_root().join("graphql/schema.graphql"))
        .expect("synthetic schema must be readable");
    let mut sources = chigiri_core::SourceMap::new();
    let source_id = sources.add("schema.graphql", &source);
    let parsed = chigiri_core::language::parse_type_system_document(
        source_id,
        &source,
        &ExecutionLimits::default(),
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let document = parsed
        .document
        .expect("The synthetic schema can be parsed.");

    let mut objects = 0;
    let mut fields = 0;
    let mut inputs = 0;
    let mut enums = 0;
    let mut scalars = 0;
    for definition in document.type_system_definitions() {
        match definition {
            TypeSystemDefinition::Object(object) => {
                objects += 1;
                fields += object.fields.len();
            }
            TypeSystemDefinition::InputObject(_) => inputs += 1,
            TypeSystemDefinition::Enum(_) => enums += 1,
            TypeSystemDefinition::Scalar(_) => scalars += 1,
            _ => {}
        }
    }

    assert_eq!(
        (objects, fields, inputs, enums, scalars),
        (71, 754, 19, 65, 5)
    );
}

#[test]
fn large_fixture_output_is_split_by_owner_and_role_within_resource_limits() {
    let temporary = Fixture::new(false);
    temporary.success("codegen");
    temporary.success("check");
    let output = temporary.join("generated");

    let files = generated_files(&output);
    assert!(!files.is_empty());
    assert!(files.len() <= 65_536);
    assert!(files.values().map(String::len).sum::<usize>() <= 64 * 1024 * 1024);
    assert!(files.values().any(|source| source.lines().count() > 400));
    assert!(files.contains_key("src/__chigiri/resolvers/query.rs"));
    assert!(files.contains_key("src/__chigiri/resolvers/mutation.rs"));
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/resolvers/objects/"))
            .count(),
        69
    );
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/projection/"))
            .count(),
        0
    );
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/types/"))
            .count(),
        19 + 65 + 1 + 1
    );
    assert!(files.contains_key("src/__chigiri/types/node.rs"));
    assert!(files.contains_key("src/__chigiri/types/search_result.rs"));
}

#[test]
fn res_01_abi8_preserves_typestate_structural_stability() {
    let temporary = Fixture::new(false);
    temporary.success("codegen");
    temporary.success("check");
    let output = temporary.join("generated");
    let files = generated_files(&output);
    let dispatch = files
        .iter()
        .filter(|(name, _)| {
            name.starts_with("src/__chigiri/internal/dispatch/")
                || name.as_str() == "src/__chigiri/assembly.rs"
        })
        .map(|(_, source)| source.as_str())
        .collect::<String>();
    let root = &files["src/lib.rs"];

    assert!(root.contains("chigiri::GENERATED_ABI_VERSION == 8"));
    assert!(dispatch.contains("pub struct ResolverStage<C, T, E, const INDEX: usize>"));
    assert!(dispatch.contains("pub struct Resolvers<C, T>"));
    assert!(dispatch.contains("std::boxed::Box<["));
    assert!(dispatch.contains("ResolverGroupDispatch<C>"));
    assert_eq!(dispatch.matches("fn object_index(&self)").count(), 71);
    assert_eq!(dispatch.matches("std::sync::Arc::new(").count(), 72);
    assert!(!dispatch.contains("Resolvers::new"));
    assert!(!dispatch.contains("Clone for ResolverStage"));
    assert!(!dispatch.contains("MaybeUninit"));
    assert!(!dispatch.contains("std::any::Any"));
    assert!(!dispatch.contains("unsafe"));

    let assembly = &files["src/__chigiri/assembly.rs"];
    assert_eq!(
        assembly
            .matches("impl<C, T, E> super::ResolverStage")
            .count(),
        71
    );
    let adapters = files
        .iter()
        .filter(|(name, _)| name.starts_with("src/__chigiri/internal/dispatch/"))
        .collect::<Vec<_>>();
    assert_eq!(adapters.len(), 71);
    assert!(
        adapters
            .iter()
            .all(|(_, source)| source.matches("fn object_index(&self)").count() == 1)
    );
}

#[test]
fn enum_2_605_enum_binding_can_be_compiled_by_consumer() {
    let temporary = Fixture::new(true);
    temporary.success("codegen");
    let before = generated_files(&temporary.join("generated"));
    let source = before
        .iter()
        .filter(|(name, _)| name.ends_with(".rs"))
        .map(|(_, source)| source.as_str())
        .collect::<String>();
    assert_eq!(source.matches("type Enum").count(), 65);
    for index in 0..65 {
        for direction in ["from", "to"] {
            let method = format!("fn enum{index:03}_{direction}_graphql(");
            assert_eq!(source.matches(&method).count(), 1, "{method}");
        }
    }
    assert!(!source.contains("pub trait Enum"));
    assert!(source.contains("pub struct Input000<T: super::SchemaTypes>"));
    assert!(source.contains("pub state: ::chigiri::Input<T::Enum000>"));
    assert!(source.contains("args: super::QueryObject000Args<T>"));
    assert!(source.contains("chigiri::FieldResult<T::Enum"));

    temporary.test_consumer();
    temporary.success("check");
    assert_eq!(before, generated_files(&temporary.join("generated")));
}
