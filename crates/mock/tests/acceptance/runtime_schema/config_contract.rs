//! Verifies the current closed configuration, embedded schema consistency,
//! exact resolver coverage, and file-size limits.

use super::{data, init, ready};
use crate::support::{Directory, Server};
use serde_json::json;
use std::fs;
use toml_edit::{DocumentMut, value};

fn reject(config: &str, phase: &str) {
    let directory = Directory::new();
    fs::write(directory.0.join("mock.toml"), config).unwrap();
    let result = directory.command(&["serve", "--config", "mock.toml"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains(&format!("[{phase}]")), "{stderr}");
    assert!(stderr.contains("mock.toml"), "{stderr}");
    assert!(!stderr.contains("PRIVATE_BODY"), "{stderr}");
    assert_eq!(
        fs::read_to_string(directory.0.join("mock.toml")).unwrap(),
        config
    );
}

fn edit(config: &str, mutation: impl FnOnce(&mut DocumentMut)) -> String {
    let mut document = config.parse::<DocumentMut>().unwrap();
    mutation(&mut document);
    document.to_string()
}

#[test]
fn cv_01_04_closed_v9_scaffold_and_exact_entries_digest_vocabulary() {
    let _test_case = crate::support::test_case();
    let (directory, scaffold) = init("type Query { answer: Int! }");
    let checked = directory.command(&["check", "--config", "mock.toml"]);
    assert!(checked.status.success());
    let config = ready(&scaffold, &[("Query.answer", "return 42")]);
    for table in ["", "server", "schema", "seed", "hooks", "state", "events"] {
        reject(
            &edit(&config, |doc| {
                if table.is_empty() {
                    doc["extra"] = value("PRIVATE_BODY");
                } else {
                    doc[table]["extra"] = value("PRIVATE_BODY");
                }
            }),
            "parse",
        );
    }
    reject(
        &edit(&config, |doc| {
            doc["schema"]["sources"]
                .as_array_of_tables_mut()
                .unwrap()
                .get_mut(0)
                .unwrap()["extra"] = value(true)
        }),
        "parse",
    );
    reject(
        &edit(&config, |doc| {
            doc["resolvers"]["Query.answer"]["extra"] = value(true)
        }),
        "parse",
    );
    reject(
        &edit(&config, |doc| {
            doc["consumers"]["unregistered"]["luau"] = value("PRIVATE_BODY")
        }),
        "events",
    );
    for key in [
        "format",
        "capabilities",
        "server",
        "schema",
        "storage",
        "clock",
        "seed",
        "resolvers",
        "hooks",
        "state",
        "events",
        "consumers",
    ] {
        reject(
            &edit(&config, |doc| {
                doc.remove(key);
            }),
            "parse",
        );
    }
    reject(
        &edit(&config, |doc| {
            doc["resolvers"]
                .as_table_mut()
                .unwrap()
                .remove("Query.answer");
        }),
        "validation",
    );
    reject(
        &edit(&config, |doc| {
            doc["resolvers"]["Query.extra"] = doc["resolvers"]["Query.answer"].clone();
        }),
        "validation",
    );
    for format in [
        "chigiri-mock/v0",
        "chigiri-mock/v1",
        "chigiri-mock/v2",
        "chigiri-mock/v3",
        "chigiri-mock/v4",
        "chigiri-mock/v5",
        "chigiri-mock/v6",
        "chigiri-mock/v7",
        "chigiri-mock/v8",
        "chigiri-mock/v9",
        "chigiri-mock/v10",
    ] {
        reject(
            &edit(&config, |doc| doc["format"] = value(format)),
            "validation",
        );
    }
    for state in ["ready", "unimplemented", "unknown", "Ready", ""] {
        reject(
            &edit(&config, |doc| {
                doc["resolvers"]["Query.answer"]["state"] = value(state)
            }),
            "parse",
        );
    }
    for (key, changed) in [
        ("digest", "invalid"),
        ("digest", &"0".repeat(64)),
        ("vocabulary", "future/v2"),
    ] {
        reject(
            &edit(&config, |doc| doc["schema"][key] = value(changed)),
            "schema",
        );
    }
    reject(
        &edit(&config, |doc| {
            doc["schema"]["sources"]
                .as_array_of_tables_mut()
                .unwrap()
                .get_mut(0)
                .unwrap()["text"] = value("type Query { changed: Int! }")
        }),
        "schema",
    );
    reject(
        &edit(&config, |doc| {
            doc["schema"]["sources"]
                .as_array_of_tables_mut()
                .unwrap()
                .get_mut(0)
                .unwrap()["name"] = value("chigiri-mock/v1.graphql")
        }),
        "schema",
    );
    reject(
        &ready(&config, &[("Query.answer", "return 'PRIVATE_BODY'")]),
        "type",
    );
    reject(
        &ready(&config, &[("Query.answer", "if then -- PRIVATE_BODY")]),
        "compile",
    );
    let mut server = Server::offline(&config);
    assert_eq!(data(&server, "{answer}"), json!({"answer":42}));
    server.stop("-TERM");
}

#[test]
fn rsm_16_18_config_regular_file_and_exact_body_and_document_limits() {
    let _test_case = crate::support::test_case();
    let (_, scaffold) = init("type Query { answer: Int! }");
    let config = ready(&scaffold, &[("Query.answer", "return 42")]);
    let mut document = config.parse::<DocumentMut>().unwrap();
    let mut module = document["resolvers"]["Query.answer"]["luau"]
        .as_str()
        .unwrap()
        .to_owned();
    module.push_str(&" ".repeat(32 * 1024 - module.len()));
    document["resolvers"]["Query.answer"]["luau"] = value(module.clone());
    let mut server = Server::start(&document.to_string());
    assert_eq!(data(&server, "{answer}"), json!({"answer":42}));
    server.stop("-TERM");
    module.push(' ');
    document["resolvers"]["Query.answer"]["luau"] = value(module);
    reject(&document.to_string(), "limit");
    let config = ready(&scaffold, &[("Query.answer", "return 42")]);
    // Account for the shorter port inserted by the startup helper before padding
    // the serialized configuration to the exact boundary.
    let config = config.replace("127.0.0.1:4000", "127.0.0.1:0");
    let padded = format!("{config}{}", " ".repeat(256 * 1024 - config.len()));
    let mut server = Server::start(&padded);
    assert_eq!(data(&server, "{answer}"), json!({"answer":42}));
    server.stop("-TERM");
    reject(&format!("{padded} "), "limit");
    let directory = Directory::new();
    fs::write(directory.0.join("mock.toml"), &config).unwrap();
    std::os::unix::fs::symlink("mock.toml", directory.0.join("link.toml")).unwrap();
    fs::write(directory.0.join("bytes.toml"), [0xff]).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(directory.0.join("fifo.toml"))
            .status()
            .unwrap()
            .success()
    );
    for path in ["link.toml", "bytes.toml", "fifo.toml", "."] {
        let result = directory.command(&["serve", "--config", path]);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("[read]"));
    }
}
