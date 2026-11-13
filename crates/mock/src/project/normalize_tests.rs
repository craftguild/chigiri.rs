use chigiri::core::Source;

use super::normalize::AdoptedSchema;

fn adopted(text: &str) -> AdoptedSchema {
    AdoptedSchema::from_sources(&[Source::new("ignored.graphql", text)]).unwrap()
}

#[test]
fn normalization_has_an_independent_expected_token_form() {
    let input = concat!(
        "\u{feff}# ignored\r\n",
        "\"Model # text\" type Query, {\n",
        " value(arg: String = \"quote: \\\" slash: \\\\ line: \\n tab: \\t ctrl: \\u0001\"): Float\n",
        "}\n",
    );
    let expected = concat!(
        "\"Model # text\" type Query { value ( arg : String = ",
        "\"quote: \\\" slash: \\\\ line: \\n tab: \\t ctrl: \\u0001\" ) : Float }\n",
    );

    assert_eq!(adopted(input).sources[0].sdl, expected);
}

#[test]
fn quoted_and_block_strings_with_equal_values_normalize_equally() {
    let quoted = adopted("\"first\\nsecond # data\" type Query { value: String }");
    let block = adopted("\"\"\"first\nsecond # data\"\"\" type Query { value: String }");

    assert_eq!(quoted.sources[0].sdl, block.sources[0].sdl);
}

#[test]
fn meaningful_values_directives_and_source_order_change_the_digest() {
    let baseline = adopted(
        "directive @tag(value: Int!) on FIELD_DEFINITION type Query { a: Int @tag(value: 1) b: Int }",
    );
    for changed in [
        "directive @tag(value: Int!) on FIELD_DEFINITION type Query { a: Int @tag(value: 2) b: Int }",
        "directive @tag(value: Int!) on FIELD_DEFINITION type Query { b: Int a: Int @tag(value: 1) }",
        "directive @tag(value: Int!) on FIELD_DEFINITION type Query { a: Int b: Int @tag(value: 1) }",
    ] {
        assert_ne!(baseline.digest, adopted(changed).digest);
    }
}

#[test]
fn normalization_is_idempotent_and_empty_input_stays_empty() {
    let first = adopted("# comment\r\ntype Query, { value: String }");
    let second = adopted(&first.sources[0].sdl);

    assert_eq!(first.sources, second.sources);
    assert_eq!(adopted("\u{feff} # only a comment\n").sources[0].sdl, "");
}

#[test]
fn public_digest_matches_the_compact_json_vector() {
    let schema = adopted("type Query { value: String }");

    assert_eq!(
        schema.digest.as_str(),
        "8d6e057c71e9f72b81dd482276cc8094911a460a687ea3f9709eb0abb3216789"
    );
}

#[test]
fn invalid_original_syntax_is_not_repaired() {
    let error = AdoptedSchema::from_sources(&[Source::new(
        "ignored.graphql",
        "type Query { value String }",
    )])
    .unwrap_err();

    assert_eq!(error.file.as_str(), "schema/001.graphql");
    assert_eq!(error.position, (1, 20));
}

#[test]
fn normalized_expansion_is_bounded_separately_from_raw_input() {
    let mut text = String::from("\"\"\"");
    text.push_str(&"x\n".repeat(crate::schema::MAX_SDL_BYTES / 2 - 20));
    text.push_str("x\"\"\" type Query { value: String }");
    assert!(text.len() <= crate::schema::MAX_SDL_BYTES);

    let error = AdoptedSchema::from_sources(&[Source::new("ignored.graphql", text)]).unwrap_err();
    assert_eq!(error.phase, "schema");
}

#[test]
fn adopted_metadata_rejects_noncanonical_payload_and_charges_json_escaping() {
    let schema = adopted("\"slash \\\\ and line \\n\" type Query { value: String }");
    let rendered = super::metadata::render(&schema, super::metadata::MAX_BYTES).unwrap();
    assert!(rendered.contains("\\\\\\\\"));
    assert!(super::metadata::render(&schema, rendered.len()).is_ok());
    assert!(super::metadata::render(&schema, rendered.len() - 1).is_err());

    let mut changed = schema.clone();
    changed.sources[0].sdl.insert_str(0, "# noncanonical\n");
    let error = changed.verify().unwrap_err();
    assert_eq!(error.phase, "ownership");
    assert_eq!(error.file, "generated/schema.snapshot.json");
}
