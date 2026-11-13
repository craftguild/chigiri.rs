use chigiri::core::{Object, Value};

use super::support::{
    capture, captured_record, fixture, project, record_output, seeded, selection, slot,
};
use crate::output::{ObjectData, OutputValue};
use crate::store::CallScope;
use crate::value::ValueBudget;

#[test]
fn captured_composite_and_hidden_keys_keep_root_fields_but_read_fresh_relations() {
    let fixture = fixture();
    let mut store = seeded(&fixture);
    let captured_snapshot = store.snapshot();
    let post = capture(
        &fixture,
        fixture.post_event,
        &record_output(&captured_snapshot, fixture.post_event, fixture.post, 0),
    );
    let user = capture(
        &fixture,
        fixture.user_event,
        &record_output(&captured_snapshot, fixture.user_event, fixture.user, 0),
    );

    let mut update = store.begin(CallScope::MutationRoot).unwrap();
    update
        .update(
            fixture.user,
            &selection(&[("tenant", "t".into()), ("code", 1.into())]),
            &Object::from_iter([("name", Value::from("fresh-user"))]),
        )
        .unwrap();
    update
        .update(
            fixture.post,
            &selection(&[("title", "captured-post".into())]),
            &Object::from_iter([("title", Value::from("fresh-post"))]),
        )
        .unwrap();
    update
        .create(
            fixture.post,
            &Object::from_iter([
                ("title", Value::from("new-post")),
                ("tenantFK", Value::from("t")),
                ("codeFK", Value::from(1)),
            ]),
        )
        .unwrap();
    let fresh = store.commit(update.prepare().unwrap()).unwrap();

    let projected_post = project(
        &post,
        fresh.clone(),
        fixture.post_event,
        &mut ValueBudget::default(),
    );
    let OutputValue::Object(projected_post_object) = &projected_post else {
        panic!("projected object required")
    };
    assert!(projected_post_object.snapshot().same_version(&fresh));
    assert!(
        !projected_post_object
            .snapshot()
            .same_version(&captured_snapshot)
    );
    let captured_post = captured_record(&projected_post);
    assert_eq!(
        captured_post
            .get(slot(&fixture, fixture.post, "title"))
            .and_then(Value::as_str),
        Some("captured-post")
    );
    assert_eq!(
        captured_post.get(slot(&fixture, fixture.post, "codeFK")),
        Some(&Value::Int(1))
    );
    let author = fresh
        .reader()
        .captured_related_output(
            fixture.ir.graph().types()[fixture.ir.models()[fixture.post.0].ty.0].fields["author"],
            captured_post,
        )
        .unwrap();
    assert_model_field(&fixture, &author, fixture.user, "name", "fresh-user");

    let projected_user = project(
        &user,
        fresh.clone(),
        fixture.user_event,
        &mut ValueBudget::default(),
    );
    let captured_user = captured_record(&projected_user);
    assert_eq!(
        captured_user
            .get(slot(&fixture, fixture.user, "name"))
            .and_then(Value::as_str),
        Some("captured-user")
    );
    let posts = fresh
        .reader()
        .captured_related_output(
            fixture.ir.graph().types()[fixture.ir.models()[fixture.user.0].ty.0].fields["posts"],
            captured_user,
        )
        .unwrap();
    let OutputValue::List(posts) = posts else {
        panic!("posts list required")
    };
    assert_eq!(posts.len(), 2);
    assert_model_field(&fixture, &posts[0], fixture.post, "title", "fresh-post");
    assert_model_field(&fixture, &posts[1], fixture.post, "title", "new-post");
}

#[test]
fn deleted_targets_fail_and_reused_composite_identity_resolves_only_the_new_record() {
    let fixture = fixture();
    let mut store = seeded(&fixture);
    let captured = capture(
        &fixture,
        fixture.post_event,
        &record_output(&store.snapshot(), fixture.post_event, fixture.post, 0),
    );
    let mut remove = store.begin(CallScope::MutationRoot).unwrap();
    remove
        .update(
            fixture.post,
            &selection(&[("title", "captured-post".into())]),
            &Object::from_iter([("tenantFK", Value::Null), ("codeFK", Value::Null)]),
        )
        .unwrap();
    remove
        .delete(
            fixture.user,
            &selection(&[("tenant", "t".into()), ("code", 1.into())]),
        )
        .unwrap();
    let missing = store.commit(remove.prepare().unwrap()).unwrap();
    let projected = project(
        &captured,
        missing.clone(),
        fixture.post_event,
        &mut ValueBudget::default(),
    );
    let relation =
        fixture.ir.graph().types()[fixture.ir.models()[fixture.post.0].ty.0].fields["author"];
    assert!(
        missing
            .reader()
            .captured_related_output(relation, captured_record(&projected))
            .is_err()
    );

    let mut recreate = store.begin(CallScope::MutationRoot).unwrap();
    recreate
        .create(
            fixture.user,
            &Object::from_iter([
                ("tenant", Value::from("t")),
                ("code", Value::from(1)),
                ("name", Value::from("replacement")),
            ]),
        )
        .unwrap();
    let fresh = store.commit(recreate.prepare().unwrap()).unwrap();
    let projected = project(
        &captured,
        fresh.clone(),
        fixture.post_event,
        &mut ValueBudget::default(),
    );
    let related = fresh
        .reader()
        .captured_related_output(relation, captured_record(&projected))
        .unwrap();
    assert_model_field(&fixture, &related, fixture.user, "name", "replacement");
}

fn assert_model_field(
    fixture: &super::support::Fixture,
    output: &OutputValue,
    model: crate::ir::ModelId,
    name: &str,
    expected: &str,
) {
    let OutputValue::Object(object) = output else {
        panic!("object required")
    };
    let ObjectData::Model(record) = object.data() else {
        panic!("fresh model required")
    };
    assert_eq!(
        record
            .get(slot(fixture, model, name))
            .and_then(Value::as_str),
        Some(expected)
    );
}
