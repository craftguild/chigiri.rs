//! Compile maintained documentation against actual generated traits and context APIs.

use super::{TestDirectory, assert_consumer_lanes, fixture, setup};
use std::fs;

fn block<'a>(document: &'a str, heading: &str, fence: &str) -> &'a str {
    document
        .split(heading)
        .nth(1)
        .unwrap()
        .split(fence)
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
}

#[test]
fn ri_02_08_adr_53_domain_methods_mapper_and_named_factory_compile() {
    let directory = TestDirectory::new("adr53-recipes");
    let adr = fs::read_to_string(fixture(
        "../../docs/adr/0053-trait-only-resolvers-and-application-boundaries.md",
    ))
    .unwrap();
    fs::create_dir_all(directory.join("schema")).unwrap();
    fs::write(
        directory.join("schema/schema.graphql"),
        block(&adr, "### 3.2 Application example", "```graphql\n"),
    )
    .unwrap();
    directory.generate();
    fs::create_dir_all(directory.join("domain/src")).unwrap();
    fs::copy(
        fixture("tests/fixtures/trait-errors/domain.rs"),
        directory.join("domain/src/lib.rs"),
    )
    .unwrap();
    fs::write(
        directory.join("domain/Cargo.toml"),
        "[package]\nname=\"domain\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    let methods = block(&adr, "### 3.2 Application example", "```rust,ignore\n");
    let conversion = block(&adr, "### 4.1 Assembly API", "```rust,ignore\n");
    let (mapper, assembly) = conversion.split_once("let resolvers =").unwrap();
    let factory = block(
        &adr,
        "## 5. Context through existing APIs",
        "```rust,ignore\n",
    );
    let source = format!(
        "struct AppContext {{ users: std::sync::Arc<domain::Users> }}\n\
         impl AppContext {{ async fn prepare(_: chigiri::HttpRequestContext<'_>) -> Result<Self, chigiri::PrepareError> {{ Ok(Self {{ users: std::sync::Arc::new(domain::Users) }}) }} }}\n\
         {methods}\n{mapper}\n{factory}\n\
         pub fn documented_assembly() {{ let resolvers = {assembly}\n\
         let _endpoint = chigiri::Endpoint::builder(resolvers).context_factory(prepare_context).build().unwrap(); }}\n"
    );
    directory.write_consumer("adr53-documentation-consumer", &source);
    let manifest = directory.join("consumer/Cargo.toml");
    let content = fs::read_to_string(&manifest).unwrap() + "domain={path=\"../domain\"}\n";
    fs::write(manifest, content).unwrap();
    directory.assert_consumer("test");
    assert_consumer_lanes(&directory);
}

#[test]
fn ri_04_06_08_documented_factories_and_real_http_use_typed_application_errors() {
    let directory = setup();
    let sources = fixture("tests/fixtures/trait-errors");
    for name in ["context.rs", "http.rs", "http_fields.rs"] {
        fs::copy(
            sources.join(name),
            directory.join(&format!("consumer/src/{name}")),
        )
        .unwrap();
    }
    let guide = fs::read_to_string(fixture("../../docs/rust-integration.md")).unwrap();
    let named = guide
        .split("```rust,ignore\nfn prepare_context")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let captured = guide
        .split("```rust,ignore\nfn with_services")
        .nth(1)
        .unwrap()
        .split("\nlet endpoint")
        .next()
        .unwrap();
    fs::write(
        directory.join("consumer/src/recipes.rs"),
        format!("fn prepare_context{named}\nfn with_services{captured}\n"),
    )
    .unwrap();
    let library = directory.join("consumer/src/lib.rs");
    let mut source = fs::read_to_string(&library).unwrap();
    source.push_str("\n#[cfg(test)]\nmod context;\n#[cfg(test)]\nmod http_fields;\n");
    fs::write(library, source).unwrap();
    let manifest = directory.join("consumer/Cargo.toml");
    let mut source = fs::read_to_string(&manifest).unwrap();
    source.push_str(concat!(
        "axum={version=\"0.8.4\",default-features=false,features=[\"http1\",\"tokio\"]}\n",
        "chigiri_axum={path=\"../../../../crates/axum\"}\nserde_json=\"1.0.140\"\n",
        "tokio={version=\"1.44.2\",default-features=false,features=[\"net\",\"rt-multi-thread\",\"macros\",\"io-util\",\"time\"]}\n",
    ));
    fs::write(manifest, source).unwrap();
    directory.assert_consumer("test");
    assert_consumer_lanes(&directory);
}
