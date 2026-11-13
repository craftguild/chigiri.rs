use super::*;

#[test]
fn root_01_data_change_from_null_to_zero_uses_a_marker() {
    let entries = parsed("root-01", &[("/body/data", Value::Null, marker("object"))]);
    assert_pass(
        "root-01",
        json!({"data": null}),
        json!({"data": {}}),
        &entries,
    );
}

#[test]
fn root_02_allows_an_empty_object_to_become_null_at_the_data_root() {
    let entries = [allow(
        "root-02",
        "/body/data",
        marker("object"),
        Value::Null,
    )];
    assert_pass(
        "root-02",
        json!({"data": {}}),
        json!({"data": null}),
        &entries,
    );
}

#[test]
fn root_03_non_empty_objects_require_root_and_all_descendant_entries() {
    let entries = parsed(
        "root-03",
        &[
            ("/body/data", Value::Null, marker("object")),
            ("/body/data/value", marker("missing"), marker("object")),
            ("/body/data/value/nested", marker("missing"), json!(1)),
        ],
    );
    assert_pass(
        "root-03",
        json!({"data": null}),
        json!({"data": {"value": {"nested": 1}}}),
        &entries,
    );
}

#[test]
fn root_04_non_empty_objects_reject_omitted_descendant_entries() {
    let entries = parsed(
        "root-04",
        &[
            ("/body/data", Value::Null, marker("object")),
            ("/body/data/value", marker("missing"), marker("object")),
            ("/body/data/value/nested", marker("missing"), json!(1)),
        ],
    );
    let old = json!({"data": null});
    let new = json!({"data": {"value": {"nested": 1}}});
    assert_fail("root-04", old.clone(), new.clone(), &entries[..1]);
    assert_fail("root-04", old, new, &entries[..2]);
}

#[test]
fn root_05_object_and_array_changes_track_root_and_child_directions() {
    let forward = parsed(
        "root-05-forward",
        &[
            ("/body/data", marker("object"), marker("array")),
            ("/body/data/0", marker("missing"), json!(2)),
            ("/body/data/name", json!(1), marker("missing")),
        ],
    );
    assert_pass(
        "root-05-forward",
        json!({"data": {"name": 1}}),
        json!({"data": [2]}),
        &forward,
    );

    let reverse = parsed(
        "root-05-reverse",
        &[
            ("/body/data", marker("array"), marker("object")),
            ("/body/data/0", json!(2), marker("missing")),
            ("/body/data/name", marker("missing"), json!(1)),
        ],
    );
    assert_pass(
        "root-05-reverse",
        json!({"data": [2]}),
        json!({"data": {"name": 1}}),
        &reverse,
    );
}

#[test]
fn root_06_distinguishes_missing_errors_from_empty_and_nonempty_arrays() {
    let empty = parsed(
        "root-06-empty",
        &[("/body/errors", marker("missing"), marker("array"))],
    );
    assert_pass("root-06-empty", json!({}), json!({"errors": []}), &empty);

    let non_empty = parsed(
        "root-06-non-empty",
        &[
            ("/body/errors", marker("array"), marker("missing")),
            ("/body/errors/0", marker("object"), marker("missing")),
            (
                "/body/errors/0/message",
                json!("failure"),
                marker("missing"),
            ),
        ],
    );
    assert_pass(
        "root-06-non-empty",
        json!({"errors": [{"message": "failure"}]}),
        json!({}),
        &non_empty,
    );
}

#[test]
fn root_07_missing_and_null_extensions_represent_object_differences() {
    let missing = parsed(
        "root-07-missing",
        &[
            ("/body/extensions", marker("missing"), marker("object")),
            ("/body/extensions/trace", marker("missing"), json!("new")),
        ],
    );
    assert_pass(
        "root-07-missing",
        json!({}),
        json!({"extensions": {"trace": "new"}}),
        &missing,
    );

    let null = parsed(
        "root-07-null",
        &[("/body/extensions", marker("object"), Value::Null)],
    );
    assert_pass(
        "root-07-null",
        json!({"extensions": {}}),
        json!({"extensions": null}),
        &null,
    );
}

#[test]
fn root_08_missing_and_scalar_null_data_use_exact_markers() {
    let scalar = parsed(
        "root-08-scalar",
        &[("/body/data", marker("missing"), json!(7))],
    );
    assert_pass("root-08-scalar", json!({}), json!({"data": 7}), &scalar);

    let null = parsed(
        "root-08-null",
        &[("/body/data", Value::Null, marker("missing"))],
    );
    assert_pass("root-08-null", json!({"data": null}), json!({}), &null);
}
