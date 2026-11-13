//! Verifies rich metadata from the checked-in generated schema through introspection.

use std::future::Future;
use std::task::{Context, Poll, Waker};

use chigiri::Value;

use super::{MutationFields, QueryFields, TeamFields, UserFields, generated};

#[test]
fn generated_descriptor_metadata_is_released_losslessly() {
    let resolvers = generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_user(UserFields)
        .with_team(TeamFields)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .build();
    let endpoint = chigiri::Endpoint::builder(resolvers)
        .inject(|components| {
            components
                .execution()
                .dangerously_enable_introspection("compile fixture metadata contract");
        })
        .without_context()
        .build()
        .unwrap();
    let response =
        block_on(endpoint.execute(chigiri::GraphQlRequest::new(QUERY), (), &NeverDeadline));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected introspection data");
    };

    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        object(&data, "scalar").get("specifiedByURL"),
        Some(&Value::from("https://example.test/datetime"))
    );
    assert_eq!(
        object(&data, "selector").get("isOneOf"),
        Some(&Value::Boolean(true))
    );

    let role = object(&data, "role");
    assert_eq!(list(role, "visible").len(), 1);
    let all_role = list(role, "all");
    let reader = named(all_role, "READER");
    assert_eq!(reader.get("isDeprecated"), Some(&Value::Boolean(true)));
    assert_eq!(
        reader.get("deprecationReason"),
        Some(&Value::from("Migrate to VIEWER"))
    );

    let date_range = object(&data, "dateRange");
    assert_eq!(list(date_range, "visible").len(), 2);
    let legacy_input = named(list(date_range, "all"), "legacy");
    assert_eq!(
        legacy_input.get("defaultValue"),
        Some(&Value::from("\"old\""))
    );
    assert_eq!(
        legacy_input.get("isDeprecated"),
        Some(&Value::Boolean(true))
    );

    let query = object(&data, "query");
    let health = named(list(query, "allFields"), "health");
    assert_eq!(
        health.get("deprecationReason"),
        Some(&Value::from("use status"))
    );
    let user = named(list(query, "allFields"), "user");
    assert_eq!(list(user, "visibleArgs").len(), 3);
    let legacy_argument = named(list(user, "allArgs"), "legacy");
    assert_eq!(
        legacy_argument.get("isDeprecated"),
        Some(&Value::Boolean(true))
    );

    let schema_value = object(&data, "schema");
    assert_eq!(
        schema_value.get("description"),
        Some(&Value::from("Generated fixture schema"))
    );
    let auth = named(list(schema_value, "directives"), "auth");
    assert_eq!(auth.get("isRepeatable"), Some(&Value::Boolean(true)));
    let legacy = named(list(auth, "args"), "legacy");
    assert_eq!(legacy.get("defaultValue"), Some(&Value::from("\"legacy\"")));
    assert_eq!(legacy.get("isDeprecated"), Some(&Value::Boolean(true)));
}

struct NeverDeadline;

impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

fn object<'a>(object: &'a chigiri::Object, key: &str) -> &'a chigiri::Object {
    object.get(key).and_then(Value::as_object).unwrap()
}

fn list<'a>(object: &'a chigiri::Object, key: &str) -> &'a [Value] {
    object.get(key).and_then(Value::as_list).unwrap()
}

fn named<'a>(values: &'a [Value], name: &str) -> &'a chigiri::Object {
    values
        .iter()
        .filter_map(Value::as_object)
        .find(|value| value.get("name") == Some(&Value::from(name)))
        .unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

const QUERY: &str = r#"
{
  scalar: __type(name: "DateTime") { specifiedByURL }
  selector: __type(name: "Selector") { isOneOf }
  role: __type(name: "Role") {
    visible: enumValues { name }
    all: enumValues(includeDeprecated: true) { name isDeprecated deprecationReason }
  }
  dateRange: __type(name: "DateRange") {
    visible: inputFields { name }
    all: inputFields(includeDeprecated: true) {
      name defaultValue isDeprecated deprecationReason
    }
  }
  query: __type(name: "Query") {
    allFields: fields(includeDeprecated: true) {
      name deprecationReason
      visibleArgs: args { name }
      allArgs: args(includeDeprecated: true) {
        name defaultValue isDeprecated deprecationReason
      }
    }
  }
  schema: __schema {
    description
    directives {
      name isRepeatable locations
      args(includeDeprecated: true) { name defaultValue isDeprecated deprecationReason }
    }
  }
}
"#;
