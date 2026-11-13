//! Test initial candidate ownership and generation identity without retaining SDL as a second authority.

use std::sync::Arc;

use chigiri::core::Source;

use super::digest::Digest;
use super::state::{Snapshot, State};
use super::{GeneratedProject, generate_project};
use crate::ir::MockIr;

fn generate(source: &str) -> GeneratedProject {
    let source = source
        .replace(" @chigiri_mock_model", "")
        .replace(" @chigiri_mock_resolver", "");
    generate_project(Arc::new(
        MockIr::parse(vec![Source::new("schema/001.graphql", &source)]).unwrap(),
    ))
    .unwrap()
}

fn metadata(project: &GeneratedProject) -> (State, Snapshot) {
    let state: State = serde_json::from_str(&project.files["generated/update-state.json"]).unwrap();
    let snapshot: Snapshot =
        serde_json::from_str(&project.files["generated/schema.snapshot.json"]).unwrap();
    (state, snapshot)
}

#[test]
fn mp_01_02_generation_has_role_modules_empty_seeds_and_verified_stable_metadata() {
    let _test_case = crate::vm_gate::test_case();
    let source = include_str!("../../fixtures/blog.graphql");
    let project = generate(source);
    assert_eq!(project.files.len(), 17);
    assert_eq!(project.files["seeds/User.jsonl"], "");
    assert_eq!(project.files["seeds/Post.jsonl"], "");
    assert!(
        project.files["resolvers/query/getUsers.luau"].contains("ChigiriQueryGetUsersResolver")
    );
    assert!(project.files["resolvers/query/getUsers.luau"].contains("NOT_IMPLEMENTED"));
    assert!(!project.files["resolvers/query/getUsers.luau"].contains("state ="));
    assert!(
        project.files["resolvers/mutation/createUser.luau"]
            .contains("ChigiriMutationCreateUserContext")
    );
    assert!(
        !project
            .files
            .keys()
            .any(|path| path.ends_with("posts.luau") || path.ends_with("author.luau"))
    );
    let config = crate::luau::format::source(
        &super::CONFIG_DECLARATIONS.replace("type Chigiri", "export type Chigiri"),
        crate::luau::MAX_WRAPPER,
    )
    .unwrap();
    assert!(project.files["generated/types.d.luau"].contains(&config));
    {
        let _lease = crate::vm_gate::test_guard();
        let definitions = &project.files["generated/types.d.luau"];
        assert!(definitions.contains("declare extern type ChigiriNull with\nend"));
        assert!(definitions.contains("export type ChigiriQueryGetUsersResolver"));
        luaur::compile(&project.files["resolvers/query/getUsers.luau"]).unwrap();
        luaur::check_with_definitions("local value: ChigiriNull? = nil", definitions).unwrap();
        assert!(
            luaur::check_with_definitions("local value: ChigiriNull = {}", definitions).is_err()
        );
        assert!(
            luaur::check_with_definitions(
                "local value: ChigiriQueryGetUsersResult = 'wrong'",
                definitions
            )
            .is_err()
        );
    }
    let (state, snapshot) = metadata(&project);
    assert_eq!(state.file_count(), 18);
    assert_eq!(state.generation, snapshot.generation);
    state
        .verify(
            &project.files["generated/types.d.luau"],
            &snapshot,
            &project.files["generated/schema.snapshot.json"],
        )
        .unwrap();
    assert!(project.files["generated/schema.snapshot.json"].contains("type User"));
    assert_eq!(snapshot.public_schema.sources.len(), 1);
    assert_eq!(snapshot.public_schema.sources[0].name, "schema/001.graphql");
    assert_eq!(state.public_digest, snapshot.public_schema.digest);
    assert!(snapshot.summary.entries.contains_key("model/User"));
    assert!(
        snapshot
            .summary
            .entries
            .contains_key("relation/Post.author")
    );
    assert_eq!(project.files, generate(source).files);
    let extended = generate(&format!(
        "{source}\nextend type Query {{ another:Int! @chigiri_mock_resolver }}"
    ));
    assert!(extended.files.contains_key("resolvers/query/another.luau"));
    for (path, bytes) in project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("resolvers/"))
    {
        assert_eq!(&extended.files[path], bytes);
    }
    let (_, next) = metadata(&extended);
    for (key, stamp) in &snapshot.summary.entries {
        assert_eq!(&next.summary.entries[key], stamp, "{key}");
    }
}

#[test]
fn mp_01_02_generation_supports_custom_roots_nested_manual_and_other_domain() {
    let _test_case = crate::vm_gate::test_case();
    let source = include_str!("../../fixtures/projects.graphql")
        .replace("Query", "Lookup")
        .replace("Mutation", "Commands")
        + "\nschema { query: Lookup mutation: Commands }";
    let project = generate(&source);
    assert!(
        project
            .files
            .keys()
            .any(|path| path.starts_with("resolvers/"))
    );
    assert!(
        project
            .files
            .contains_key("resolvers/objects/Member/label.luau")
    );
    assert!(project.files.contains_key("seeds/Task.jsonl"));
    assert!(
        project
            .files
            .keys()
            .any(|path| path.starts_with("resolvers/query/"))
    );
    assert!(
        project
            .files
            .keys()
            .any(|path| path.starts_with("resolvers/mutation/"))
    );
    let (state, snapshot) = metadata(&project);
    state
        .verify(
            &project.files["generated/types.d.luau"],
            &snapshot,
            &project.files["generated/schema.snapshot.json"],
        )
        .unwrap();
    assert_eq!(snapshot.summary.query, "Lookup");
}

#[test]
fn mp_13_metadata_rejects_missing_base_unknown_fields_drift_generation_and_path_escape() {
    let _test_case = crate::vm_gate::test_case();
    let project = generate(include_str!("../../fixtures/blog.graphql"));
    let (state, snapshot) = metadata(&project);
    let state_json = &project.files["generated/update-state.json"];
    for change in ["missing", "unknown", "digest"] {
        let mut value: serde_json::Value = serde_json::from_str(state_json).unwrap();
        match change {
            "missing" => {
                value["resolvers"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("base");
            }
            "unknown" => {
                value["resolvers"][0]["extra"] = true.into();
            }
            _ => value["resolvers"][0]["base"] = "bad digest".into(),
        }
        assert!(serde_json::from_str::<State>(&value.to_string()).is_err());
    }
    assert!(
        state
            .verify(
                "edited types",
                &snapshot,
                &project.files["generated/schema.snapshot.json"]
            )
            .is_err()
    );
    let mut changed = state.clone();
    changed.generation = Digest::bytes(b"different");
    assert!(
        changed
            .verify(
                &project.files["generated/types.d.luau"],
                &snapshot,
                &project.files["generated/schema.snapshot.json"]
            )
            .is_err()
    );
    for path in [
        "../escape.luau",
        "/private/absolute.luau",
        "resolvers/query/../value.luau",
    ] {
        let mut changed = serde_json::to_value(&state).unwrap();
        changed["resolvers"][0]["path"] = path.into();
        assert!(serde_json::from_value::<State>(changed).is_err());
    }
    let digest = Digest::bytes(b"abc");
    assert_eq!(
        serde_json::to_string(&digest).unwrap(),
        "\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\""
    );
}

#[test]
fn mp_13_31_metadata_entries_are_bounded_and_snapshot_facts_are_unique_closed_keys() {
    let digest = serde_json::to_string(&Digest::bytes(b"x")).unwrap();
    for (count, accepted) in [(1024, true), (1025, false)] {
        let json = format!("[{}]", vec![digest.clone(); count].join(","));
        assert_eq!(
            serde_json::from_str::<super::metadata::Entries<Digest>>(&json).is_ok(),
            accepted
        );
    }
    for facts in [
        format!("\"type/X\":{digest},\"type/X\":{digest}"),
        format!("\"/private/path\":{digest}"),
    ] {
        let source = format!("{{\"query\":\"Query\",\"mutation\":null,\"entries\":{{{facts}}}}}");
        assert!(serde_json::from_str::<super::summary::Summary>(&source).is_err());
    }
}
