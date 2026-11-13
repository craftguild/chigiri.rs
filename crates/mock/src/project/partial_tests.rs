//! Verify partial checks do not grant startup readiness or implicitly adopt dependent candidates.

use super::test_support::Fixture;
use super::{ActiveKind, ActiveSnapshot, Readiness};
use crate::luau::render_module;

const SDL: &str = include_str!("../../fixtures/blog.graphql");
const USER: &str = "{\"id\":\"u1\",\"name\":\"A\"}\n";
const POST: &str = "{\"id\":\"p1\",\"title\":\"T\",\"body\":\"B\",\"authorId\":\"u1\"}\n";

fn replacement(path: &str, source: &str) -> (String, String) {
    (path.into(), source.into())
}

fn resolver(snapshot: &ActiveSnapshot, path: &str, body: &str) -> (String, String) {
    let file = snapshot
        .files()
        .iter()
        .find(|file| file.path() == path)
        .unwrap();
    let ActiveKind::Resolver(id) = file.kind() else {
        panic!("resolver source required");
    };
    replacement(
        path,
        &render_module(snapshot.schema().ir(), id, body).unwrap(),
    )
}

#[test]
fn ai_15_16_partial_resolver_improves_invalid_module_with_unrelated_invalid_seed_preserved() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let path = "resolvers/query/getUsers.luau";
    fixture.write(path, "invalid source");
    fixture.write("seeds/User.jsonl", "invalid JSON");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let ready = resolver(&snapshot, path, "return ctx.models[\"User\"]:find({})");
    assert!(matches!(
        snapshot.validate_selection(&[ready]).unwrap(),
        Readiness::NotReady(_)
    ));
    assert_eq!(fixture.read(path), "invalid source");
    assert_eq!(fixture.read("seeds/User.jsonl"), "invalid JSON");
    assert!(snapshot.check().is_err());
}

#[test]
fn ai_16_selected_modules_require_expected_closed_type_and_do_not_resolve() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { first: Int! @chigiri_mock_resolver second: Int! @chigiri_mock_resolver }",
    );
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let path = "resolvers/query/first.luau";
    let original = fixture.read(path);
    assert!(matches!(
        snapshot
            .validate_selection(&[replacement(path, &original)])
            .unwrap(),
        Readiness::Ready
    ));
    let invalid = resolver(&snapshot, path, "return \"wrong result\"");
    assert!(snapshot.validate_selection(&[invalid]).is_err());
    let cast = replacement(
        path,
        "return {resolve = function() return 1 end, extra = true} :: any",
    );
    assert!(snapshot.validate_selection(&[cast]).is_err());
    let infinite_body = resolver(&snapshot, path, "while true do end\nreturn 1");
    assert!(matches!(
        snapshot.validate_selection(&[infinite_body]).unwrap(),
        Readiness::Ready
    ));
    let first = resolver(&snapshot, path, "return 1");
    let second = resolver(&snapshot, "resolvers/query/second.luau", "return 2");
    assert!(matches!(
        snapshot.validate_selection(&[first, second]).unwrap(),
        Readiness::Ready
    ));
    assert_eq!(fixture.read(path), original);
}

#[test]
fn ai_17_connected_seed_checks_retained_current_files_and_ignores_unrelated_invalid_files() {
    let _test_case = crate::vm_gate::test_case();
    let sdl = format!(
        "{SDL}\ntype Separate @chigiri_mock_model(key: \"id\") {{ id: ID! name: String! }}"
    );
    let fixture = Fixture::new(&sdl);
    fixture.write("seeds/User.jsonl", USER);
    fixture.write("seeds/Separate.jsonl", "invalid JSON");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let seed = replacement("seeds/Post.jsonl", POST);
    assert!(matches!(
        snapshot.validate_selection(&[seed]).unwrap(),
        Readiness::NotReady(_)
    ));
    let seed = replacement("seeds/Post.jsonl", &POST.replace("u1", "absent"));
    let error = snapshot.validate_selection(&[seed]).err().unwrap();
    assert_eq!(error.message, "unresolved relation reference");
    assert_eq!(error.file, "seeds/Post.jsonl");
    fixture.write("seeds/User.jsonl", "invalid JSON");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let seed = replacement("seeds/Post.jsonl", POST);
    assert!(snapshot.validate_selection(&[seed]).is_err());
    assert_eq!(fixture.read("seeds/Separate.jsonl"), "invalid JSON");
}

#[test]
fn sa_10_target_seed_checks_explicit_reference_dependencies_only() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type User{id:ID! name:String!} type Copy{id:ID! user:User!} type Separate{id:ID! name:String!} type Query{copies:[Copy!]!}",
    );
    fixture.write("seeds/User.jsonl", "invalid dependency");
    fixture.write("seeds/Separate.jsonl", "invalid unrelated seed");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let copy = replacement(
        "seeds/Copy.jsonl",
        "{\"id\":\"c\",\"user\":{\"$ref\":{\"model\":\"User\",\"key\":{\"id\":\"u\"}}}}\n",
    );
    let error = snapshot
        .validate_selection(std::slice::from_ref(&copy))
        .err()
        .unwrap();
    assert_eq!(error.file, "seeds/User.jsonl");

    fixture.write("seeds/User.jsonl", "{\"id\":\"u\",\"name\":\"Akari\"}\n");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert!(matches!(
        snapshot.validate_selection(&[copy]).unwrap(),
        Readiness::NotReady(_)
    ));
    assert_eq!(
        fixture.read("seeds/Separate.jsonl"),
        "invalid unrelated seed"
    );
}

#[test]
fn ai_18_individually_valid_candidates_can_form_an_invalid_adoption_set() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    fixture.write(
        "seeds/User.jsonl",
        &(USER.to_owned() + &USER.replace("u1", "u2")),
    );
    fixture.write("seeds/Post.jsonl", POST);
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let user = replacement("seeds/User.jsonl", USER);
    let post = replacement("seeds/Post.jsonl", &POST.replace("u1", "u2"));
    match snapshot
        .validate_selection(std::slice::from_ref(&user))
        .unwrap()
    {
        Readiness::Ready => {}
        Readiness::NotReady(error) | Readiness::NotEvaluated(error) => {
            panic!("user candidate not ready: {}", error.message)
        }
    }
    assert!(matches!(
        snapshot
            .validate_selection(std::slice::from_ref(&post))
            .unwrap(),
        Readiness::Ready
    ));
    assert_eq!(
        snapshot
            .validate_selection(&[user, post])
            .err()
            .unwrap()
            .message,
        "unresolved relation reference"
    );
    assert!(fixture.read("seeds/User.jsonl").contains("u2"));
    assert_eq!(fixture.read("seeds/Post.jsonl"), POST);
}

#[test]
fn ai_18_whole_check_environment_failure_is_not_evaluated_and_keeps_required_validation() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Query { first: Int! @chigiri_mock_resolver }");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let view = snapshot.validation().unwrap();
    let ready = resolver(&snapshot, "resolvers/query/first.luau", "return 1");
    let environment = || {
        Err(super::read::check_error(
            snapshot.schema().ir(),
            &snapshot.layout,
            crate::checked::CheckError::Environment("cannot create checking thread"),
        ))
    };
    let Readiness::NotEvaluated(error) =
        view.validate_selection_with(&[ready], environment).unwrap()
    else {
        panic!("environment failures must remain unevaluated readiness results");
    };
    assert_eq!(
        (error.exit, error.phase, error.message),
        (2, "environment", "cannot create checking thread")
    );
    let invalid = resolver(&snapshot, "resolvers/query/first.luau", "return \"wrong\"");
    assert!(
        view.validate_selection_with(&[invalid], || panic!(
            "full validation must stop after a required check fails"
        ))
        .is_err()
    );
    snapshot.check().unwrap();
}

#[test]
fn ai_18_whole_check_resource_failures_never_become_partial_success() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new("type Query { first: Int! @chigiri_mock_resolver }");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    let view = snapshot.validation().unwrap();
    let ready = resolver(&snapshot, "resolvers/query/first.luau", "return 1");
    for phase in ["limit", "memory", "budget", "timeout", "cancelled"] {
        let error = view
            .validate_selection_with(std::slice::from_ref(&ready), || {
                Err(super::ProjectError::new(phase, "resource failure"))
            })
            .err()
            .unwrap();
        assert_eq!(error.phase, phase);
    }
}

#[test]
fn ai_17_seed_validation_reuses_duplicate_keys_ids_presence_and_precision_rules() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    for seed in [
        "{\"id\":\"u1\",\"id\":\"u2\",\"name\":\"A\"}\n".to_owned(),
        USER.to_owned() + USER,
        "{\"id\":\"u1\"}\n".to_owned(),
        "{\"id\":\"u1\",\"name\":null}\n".to_owned(),
        "{\"id\":\"u1\",\"name\":\"A\",\"posts\":[]}\n".to_owned(),
        "\n".to_owned(),
    ] {
        assert!(
            snapshot
                .validate_selection(&[replacement("seeds/User.jsonl", &seed)])
                .is_err()
        );
    }
    let fixture = Fixture::new(
        "type Sample @chigiri_mock_model(key: \"id\") { id: ID! value: Float! } type Query { samples: [Sample!]! @chigiri_mock_crud(action: FIND, model: \"Sample\") }",
    );
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert!(
        snapshot
            .validate_selection(&[replacement(
                "seeds/Sample.jsonl",
                "{\"id\":\"s\",\"value\":9007199254740993}\n"
            )])
            .is_err()
    );
}

#[test]
fn ai_22_23_replacements_are_closed_and_share_all_retained_seed_bytes() {
    let _test_case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    for path in [
        "mock.luau",
        "generated/types.d.luau",
        "../schema.graphql",
        "seeds/Unknown.jsonl",
        "resolvers/query/getUsers.luau.dist",
    ] {
        assert!(
            snapshot
                .validate_selection(&[replacement(path, "changed")])
                .is_err()
        );
    }
    assert!(
        snapshot
            .validate_selection(&[
                replacement("seeds/User.jsonl", USER),
                replacement("seeds/User.jsonl", USER)
            ])
            .is_err()
    );
    fixture.write("seeds/Post.jsonl", &"x".repeat(crate::limits::SEED_BYTES));
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert_eq!(
        snapshot
            .validate_selection(&[replacement("seeds/User.jsonl", USER)])
            .err()
            .unwrap()
            .phase,
        "limit"
    );
}
