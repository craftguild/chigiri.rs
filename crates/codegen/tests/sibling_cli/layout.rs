//! Verify real CLI layout, read-only behavior under resource exhaustion, and drift detection.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::fixture::{Fixture, report};

#[test]
fn sg_layout_01_real_cli_generates_updates_and_checks_outputs_over_400_lines() {
    let fixture = Fixture::new();
    let mut fields = String::new();
    for index in 0..80 {
        writeln!(fields, "field{index}: String!").unwrap();
    }
    let mut input = String::new();
    for index in 0..420 {
        writeln!(input, "field{index}: Int").unwrap();
    }
    let schema = format!(
        "schema {{ query: Read mutation: Write }} type Read {{ {fields} }} type Write {{ {fields} }} input Large {{ {input} }}"
    );
    fs::write(fixture.root.join("graphql/schema.graphql"), &schema).unwrap();
    fixture.success("codegen");
    let initial = fixture.snapshot();
    for path in [
        "src/__chigiri/resolvers/query.rs",
        "src/__chigiri/resolvers/mutation.rs",
        "src/__chigiri/types/large.rs",
    ] {
        assert!(
            initial[Path::new(path)]
                .split(|byte| *byte == b'\n')
                .count()
                > 400
        );
    }
    fixture.success("check");
    assert_eq!(fixture.snapshot(), initial);
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        format!("{schema}\nenum Unrelated {{ A B }}"),
    )
    .unwrap();
    fixture.success("update");
    fixture.success("check");
    let updated = fixture.snapshot();
    for path in [
        "src/__chigiri/resolvers/query.rs",
        "src/__chigiri/resolvers/mutation.rs",
        "src/__chigiri/types/large.rs",
    ] {
        assert_eq!(initial[Path::new(path)], updated[Path::new(path)]);
    }
    assert!(updated.contains_key(Path::new("src/__chigiri/types/unrelated.rs")));
    for mutation in ["changed", "deleted", "extra"] {
        let path = fixture.output().join(if mutation == "extra" {
            "src/unowned.rs"
        } else {
            "src/__chigiri/types/large.rs"
        });
        match mutation {
            "deleted" => fs::remove_file(&path).unwrap(),
            _ => fs::write(&path, "// Owned for testing.treeIt's not working.\n").unwrap(),
        }
        let corrupt = fixture.snapshot();
        let output = fixture.run("check");
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(report(&output)["diagnostics"][0]["code"], "SF4020");
        assert_eq!(fixture.snapshot(), corrupt);
        if mutation == "extra" {
            fs::remove_file(path).unwrap();
        } else {
            fs::write(path, &updated[Path::new("src/__chigiri/types/large.rs")]).unwrap();
        }
    }
}

#[test]
fn sg_layout_06_update_over_output_estimate_limit_preserves_the_old_tree() {
    let fixture = Fixture::new();
    fixture.success("codegen");
    let initial = fixture.snapshot();
    let values = std::iter::repeat_n("0", 10_000)
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        format!("type Query {{ value: String }} input Huge {{ values: [Int] = [{values}] }}"),
    )
    .unwrap();
    let output = fixture.run("update");
    assert_eq!(output.status.code(), Some(1));
    let report = report(&output);
    assert_eq!(report["diagnostics"][0]["code"], "SF4010");
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("bytes=67108864")
    );
    assert_eq!(fixture.snapshot(), initial);
    assert_eq!(
        fs::read_dir(fixture.output().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn sg_layout_05_rejects_normalized_directive_path_collisions_and_preserves_the_old_tree() {
    let fixture = Fixture::new();
    fixture.success("codegen");
    let initial = fixture.snapshot();
    fs::write(fixture.root.join("graphql/schema.graphql"), "directive @aBC on FIELD_DEFINITION\ndirective @a_bc on FIELD_DEFINITION\ntype Query { value: String }\n").unwrap();
    let output = fixture.run("update");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report(&output)["diagnostics"][0]["code"], "SF4006");
    assert_eq!(fixture.snapshot(), initial);
}
