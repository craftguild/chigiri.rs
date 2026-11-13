//! Verify CLI initialization boundaries, declaration rejection, determinism, and pre-storage resource limits.

use crate::support::Directory;
use std::fs;
use std::process::Output;

const ROOT: &str = "type Query { answer: Int! }";
const INTENTIONAL_ERROR: &str = r#"        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })"#;

fn implemented(source: &str) -> String {
    assert_eq!(source.matches(INTENTIONAL_ERROR).count(), 1);
    source.replace(INTENTIONAL_ERROR, "        return {}")
}

fn run(directory: &Directory, paths: &[String], output: &str) -> Output {
    let mut args = vec!["init"];
    for path in paths {
        args.extend(["--schema", path]);
    }
    args.extend(["--config-dir", output]);
    directory.command(&args)
}

fn rejected(directory: &Directory, paths: &[String], phase: &str, exit: i32) {
    for output in ["absent.toml", "existing.toml"] {
        fs::write(directory.0.join("existing.toml"), "keep this work").unwrap();
        let result = run(directory, paths, output);
        let (exit, phase) = if output == "existing.toml" && phase != "usage" {
            (1, "ownership")
        } else {
            (exit, phase)
        };
        assert_eq!(result.status.code(), Some(exit));
        assert!(result.stdout.is_empty());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(stderr.contains(&format!("[{phase}]")), "{stderr}");
        assert!(!stderr.contains("PRIVATE_SCHEMA_BODY"), "{stderr}");
        assert!(!directory.0.join("absent.toml").exists());
        assert_eq!(
            fs::read_to_string(directory.0.join("existing.toml")).unwrap(),
            "keep this work"
        );
    }
}

#[test]
fn rsm_01_02_03_04_invalid_sdl_semantics_never_create_or_replace_output() {
    let _test_case = crate::support::test_case();
    let model = "type Item @chigiri_mock_model { id: ID! peer: Item @chigiri_mock_relation(key: \"peerId\") }";
    let variants = [
        "type Query { broken: }".to_owned(),
        format!("schema {{ query: Query subscription: Live }} {ROOT} scalar Live"),
        format!("{ROOT} scalar Secret @representation(type: STRING, min: 0)"),
        format!("{ROOT} scalar Secret @representation(type: INTEGER, min: 4, max: 2)"),
        format!("{ROOT} type Item @chigiri_mock_model {{ id: Int! }}"),
        format!(
            "{ROOT} type Item @chigiri_mock_model {{ id: ID! peerId: ID peer: Item @chigiri_mock_relation(key: \"peerId\") }}"
        ),
        format!(
            "{ROOT} type Item @chigiri_mock_model {{ id: ID! peer: Item @chigiri_mock_relation(key: \"other\") @chigiri_mock_resolver }}"
        ),
        format!(
            "{ROOT} {model} type Other @chigiri_mock_model {{ id: ID! items: [Item!]! @chigiri_mock_inverse(of: \"Item.absent\") }}"
        ),
        format!(
            "{ROOT} {model} type Other @chigiri_mock_model {{ id: ID! items: [Item!]! @chigiri_mock_inverse(of: \"Item.peer\") }}"
        ),
        format!(
            "{model} type Query {{ items: [Item!]! @chigiri_mock_crud(action: CREATE, model: \"Item\") }}"
        ),
        format!(
            "{model} type Query {{ items: [Item!]! @chigiri_mock_crud(action: FIND, model: \"Absent\") }}"
        ),
        format!("{ROOT} type Unknown @chigiri_mock_unknown {{ id: ID }}"),
        format!("{ROOT} directive @chigiri_mock_resolver(x: String) on FIELD_DEFINITION"),
    ];
    for source in variants {
        let directory = Directory::new();
        fs::write(
            directory.0.join("bad.graphql"),
            format!("# PRIVATE_SCHEMA_BODY\n{source}"),
        )
        .unwrap();
        rejected(&directory, &["bad.graphql".into()], "schema", 1);
    }
}

#[test]
fn rsm_01_valid_subscription_initializes_with_current_entrypoints() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    fs::write(
        directory.0.join("source.graphql"),
        format!("schema {{ query: Query subscription: Live }} {ROOT} type Live {{ event: Int }}"),
    )
    .unwrap();
    let output = run(&directory, &["source.graphql".into()], "mock");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let module =
        fs::read_to_string(directory.0.join("mock/resolvers/subscription/event.luau")).unwrap();
    assert!(module.contains("ChigiriLiveEventResolver"));
    assert!(module.contains("subscribe = function("));
    assert!(!module.contains("resolve = function("));
}

#[test]
fn rsm_01_16_init_regular_utf8_distinct_files_and_source_limits() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    fs::write(directory.0.join("root.graphql"), ROOT).unwrap();
    fs::hard_link(
        directory.0.join("root.graphql"),
        directory.0.join("hard.graphql"),
    )
    .unwrap();
    rejected(
        &directory,
        &["root.graphql".into(), "hard.graphql".into()],
        "schema",
        1,
    );
    rejected(
        &directory,
        &["root.graphql".into(), "root.graphql".into()],
        "schema",
        1,
    );
    std::os::unix::fs::symlink("root.graphql", directory.0.join("link.graphql")).unwrap();
    fs::write(directory.0.join("bytes.graphql"), [0xff, 0xfe]).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(directory.0.join("fifo.graphql"))
            .status()
            .unwrap()
            .success()
    );
    for path in ["link.graphql", "bytes.graphql", "fifo.graphql", "."] {
        rejected(&directory, &[path.into()], "read", 1);
    }
    rejected(&directory, &["missing.graphql".into()], "read", 2);
    assert!(
        run(&directory, &["root.graphql".into()], "probe")
            .status
            .success()
    );
    let storage_bytes = fs::read(directory.0.join("probe/storage.graphql"))
        .unwrap()
        .len();
    let max = 128 * 1024 - storage_bytes;
    let padded = format!("{ROOT}{}", " ".repeat(max - ROOT.len()));
    fs::write(directory.0.join("max.graphql"), &padded).unwrap();
    assert!(
        run(&directory, &["max.graphql".into()], "max.toml")
            .status
            .success()
    );
    fs::write(directory.0.join("over.graphql"), format!("{padded} ")).unwrap();
    rejected(&directory, &["over.graphql".into()], "limit", 1);
    fs::write(directory.0.join("extra.graphql"), " ").unwrap();
    rejected(
        &directory,
        &["max.graphql".into(), "extra.graphql".into()],
        "limit",
        1,
    );
    let paths = (0..65)
        .map(|n| {
            let name = format!("source-{n}.graphql");
            fs::write(
                directory.0.join(&name),
                if n == 0 {
                    ROOT.to_owned()
                } else {
                    format!("scalar S{n} @representation(type: STRING)")
                },
            )
            .unwrap();
            name
        })
        .collect::<Vec<_>>();
    assert!(run(&directory, &paths[..63], "63.toml").status.success());
    rejected(&directory, &paths[..64], "schema", 1);
    rejected(&directory, &paths, "usage", 2);
}

#[test]
fn as_01_ordered_normalized_sources_are_path_independent_and_description_preserving() {
    let _test_case = crate::support::test_case();
    let sources = [
        "# '''\r\n\"Item # docs\"\ntype Item { id: ID! }\n",
        "type Query { items: [Item!]! }\n",
    ];
    let normalized = [
        "\"Item # docs\" type Item { id : ID ! }\n",
        "type Query { items : [ Item ! ] ! }\n",
    ];
    let first = Directory::new();
    let second = Directory::new();
    for (directory, names) in [
        (&first, ["one.graphql", "two.graphql"]),
        (&second, ["renamed.txt", "another.txt"]),
    ] {
        for (name, source) in names.into_iter().zip(sources) {
            fs::write(directory.0.join(name), source).unwrap();
        }
        let result = run(directory, &names.map(str::to_owned), "mock");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for directory in [&first, &second] {
        let path = directory.0.join("mock/resolvers/query/items.luau");
        let resolver = implemented(&fs::read_to_string(&path).unwrap());
        fs::write(
            directory.0.join("mock/resolvers/query/items.luau"),
            resolver,
        )
        .unwrap();
        assert!(
            directory
                .command(&["export", "--config-dir", "mock", "--output", "mock.toml"])
                .status
                .success()
        );
    }
    let config = fs::read_to_string(first.0.join("mock.toml")).unwrap();
    assert_eq!(
        config,
        fs::read_to_string(second.0.join("mock.toml")).unwrap()
    );
    let parsed = config.parse::<toml_edit::DocumentMut>().unwrap();
    let embedded = parsed["schema"]["sources"].as_array_of_tables().unwrap();
    for (index, source) in normalized.iter().enumerate() {
        assert_eq!(
            embedded.get(index).unwrap()["name"].as_str().unwrap(),
            format!("schema/{:03}.graphql", index + 1)
        );
        assert_eq!(
            embedded.get(index).unwrap()["text"].as_str().unwrap(),
            *source
        );
    }
    assert!(
        run(
            &first,
            &["two.graphql".into(), "one.graphql".into()],
            "reverse"
        )
        .status
        .success()
    );
    let reverse_path = first.0.join("reverse/resolvers/query/items.luau");
    let reverse_resolver = implemented(&fs::read_to_string(&reverse_path).unwrap());
    fs::write(reverse_path, reverse_resolver).unwrap();
    assert!(
        first
            .command(&[
                "export",
                "--config-dir",
                "reverse",
                "--output",
                "reverse.toml"
            ])
            .status
            .success()
    );
    let reversed = fs::read_to_string(first.0.join("reverse.toml")).unwrap();
    assert_ne!(config, reversed);
    assert_ne!(
        parsed["schema"]["digest"].as_str(),
        reversed.parse::<toml_edit::DocumentMut>().unwrap()["schema"]["digest"].as_str()
    );
}
