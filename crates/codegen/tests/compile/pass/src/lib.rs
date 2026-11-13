#![deny(warnings)]

pub use ::generated;

#[cfg(test)]
mod governance;
#[cfg(test)]
mod introspection;
#[cfg(test)]
mod rate;

struct Types;

struct DateTime(String);
struct User;
struct Team;

impl chigiri::Scalar for DateTime {
    fn parse(value: chigiri::Value) -> Result<Self, chigiri::ScalarCoercionError> {
        match value {
            chigiri::Value::String(value) => Ok(Self(value)),
            _ => Err(chigiri::ScalarCoercionError::public(
                "DateTime needs a string.",
            )),
        }
    }

    fn serialize(&self) -> chigiri::FieldResult<chigiri::Value> {
        Ok(chigiri::Value::String(self.0.clone()))
    }
}

impl generated::SchemaTypes for Types {
    type DateTime = DateTime;
    type User = User;
    type Team = Team;
}

struct UserFields;

impl generated::UserResolver<(), Types, chigiri::FieldError> for UserFields {
    async fn id<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a User,
    ) -> chigiri::FieldResult<chigiri::Id> {
        Ok("user-1".into())
    }

    async fn name<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a User,
    ) -> chigiri::FieldResult<String> {
        Ok("Ada".to_owned())
    }

    async fn updated_at<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a User,
    ) -> chigiri::FieldResult<DateTime> {
        Ok(DateTime("2026-08-26T00:00:00Z".to_owned()))
    }
}

struct TeamFields;

impl generated::TeamResolver<(), Types, chigiri::FieldError> for TeamFields {
    async fn id<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a Team,
    ) -> chigiri::FieldResult<chigiri::Id> {
        Ok("team-1".into())
    }

    async fn members<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a Team,
        _args: generated::TeamMembersArgs,
    ) -> chigiri::FieldResult<Vec<User>> {
        Ok(vec![User])
    }
}

struct QueryFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn user<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::QueryUserArgs,
    ) -> chigiri::FieldResult<Option<User>> {
        Ok(Some(User))
    }

    async fn search<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::QuerySearchArgs,
    ) -> chigiri::FieldResult<Vec<generated::SearchResult<Types>>> {
        Ok(vec![generated::SearchResult::Team(Team)])
    }

    async fn node<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<Option<generated::Node<Types>>> {
        Ok(Some(generated::Node::User(User)))
    }

    async fn health<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<bool> {
        Ok(true)
    }
}

struct MutationFields;

impl generated::MutationResolver<(), Types, chigiri::FieldError> for MutationFields {
    async fn rename<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::MutationRenameArgs<Types>,
    ) -> chigiri::FieldResult<User> {
        Ok(User)
    }
}

pub fn build_resolvers() {
    let _ =
        generated::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>)
            .with_user(UserFields)
            .with_team(TeamFields)
            .with_query(QueryFields)
            .with_mutation(MutationFields)
            .build();
    assert_eq!(generated::SCHEMA_DESCRIPTOR.roots.query, "Query");
}

#[cfg(test)]
mod direct {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use chigiri::__private::{ArgumentValues, ResolvedObject, ResolvedValue, ResolverDispatch};
    use chigiri::Value;
    use chigiri::core::executable::CoercedInput;

    use super::{MutationFields, QueryFields, TeamFields, Types, User, UserFields, generated};

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::yield_now();
        }
    }

    fn resolvers() -> generated::Resolvers<(), Types> {
        generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
            .with_user(UserFields)
            .with_team(TeamFields)
            .with_query(QueryFields)
            .with_mutation(MutationFields)
            .build()
    }

    struct PassPlugin;

    impl chigiri::Plugin<()> for PassPlugin {}

    #[test]
    fn ordinary_async_trait_fields_execute() {
        let endpoint = chigiri::Endpoint::builder(resolvers())
            .without_context()
            .build()
            .unwrap();
        let response = block_on(endpoint.execute(
            chigiri::GraphQlRequest::new(r#"{ user(id: "user-1") { id name updatedAt } }"#),
            (),
            &NeverDeadline,
        ));
        assert!(response.errors.is_empty(), "{:?}", response.errors);
    }

    #[test]
    fn complete_traits_build_one_abi_8_assembly() {
        let endpoint = chigiri::Endpoint::builder(resolvers())
            .without_context()
            .build()
            .unwrap();
        assert_eq!(endpoint.schema_info().generated_abi(), Some(8));
    }

    fn resolve(
        resolvers: &generated::Resolvers<(), Types>,
        parent_type: &'static str,
        field_name: &'static str,
        parent: Option<&ResolvedObject>,
        values: Vec<CoercedInput>,
    ) -> ResolvedValue {
        block_on(resolvers.resolve(
            &(),
            parent_type,
            field_name,
            parent,
            ArgumentValues::new(values),
        ))
        .unwrap()
    }

    #[test]
    fn converts_enum_inputs_and_object_outputs_at_dynamic_boundaries() {
        let value = resolve(
            &resolvers(),
            "Query",
            "user",
            None,
            vec![
                CoercedInput::Value(Value::String("user-1".to_owned())),
                CoercedInput::Undefined,
                CoercedInput::Value(Value::String("READER".to_owned())),
                CoercedInput::Value(Value::String("old".to_owned())),
            ],
        );
        let ResolvedValue::Object(object) = value else {
            panic!("expected a user object.");
        };
        assert_eq!(object.type_name(), "User");
    }

    #[test]
    fn interface_and_union_lists_include_concrete_typenames() {
        let node = resolve(&resolvers(), "Query", "node", None, vec![]);
        let ResolvedValue::Object(node) = node else {
            panic!("Node output needs to be a concrete object.");
        };
        assert_eq!(node.type_name(), "User");

        let selector = Value::Object(
            [("id", Value::String("user-1".to_owned()))]
                .into_iter()
                .collect(),
        );
        let search = resolve(
            &resolvers(),
            "Query",
            "search",
            None,
            vec![CoercedInput::Value(selector)],
        );
        let ResolvedValue::List(items) = search else {
            panic!("expected a SearchResult list.");
        };
        let ResolvedValue::Object(team) = items.into_iter().next().unwrap().unwrap() else {
            panic!("The union item needs to be an object.");
        };
        assert_eq!(team.type_name(), "Team");
    }

    #[test]
    fn endpoint_builder_executes_directly() {
        let endpoint = chigiri::Endpoint::builder(resolvers())
            .execution_limits(chigiri::ExecutionLimits::production())
            .push_plugin(PassPlugin)
            .inject(|components| {
                components
                    .execution()
                    .dangerously_enable_introspection("compile fixture direct contract");
            })
            .without_context()
            .build()
            .unwrap();
        let cloned = endpoint.clone();
        let response = block_on(cloned.execute(
            chigiri::GraphQlRequest::new("{ health __typename }"),
            (),
            &NeverDeadline,
        ));

        assert!(response.errors.is_empty());
        let data = response
            .data
            .and_then(|value| match value {
                Value::Object(value) => Some(value),
                _ => None,
            })
            .unwrap();
        assert_eq!(data.get("health"), Some(&Value::Boolean(true)));
        assert_eq!(data.get("__typename"), Some(&Value::from("Query")));
        assert_eq!(
            endpoint.configuration().introspection_enabled().effective(),
            &true
        );
    }

    struct NeverDeadline;

    impl chigiri::DeadlineFactory for NeverDeadline {
        fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
            Box::pin(std::future::pending())
        }
    }

    #[test]
    fn nested_input_custom_scalar_and_scalar_output_converts() {
        let range = Value::Object(
            [
                ("from", Value::String("2026-08-26T00:00:00Z".to_owned())),
                ("legacy", Value::String("old".to_owned())),
            ]
            .into_iter()
            .collect(),
        );
        let input = Value::Object(
            [
                ("id", Value::String("user-1".to_owned())),
                ("name", Value::String("Ada".to_owned())),
                ("range", range),
            ]
            .into_iter()
            .collect(),
        );
        let renamed = resolve(
            &resolvers(),
            "Mutation",
            "rename",
            None,
            vec![CoercedInput::Value(input)],
        );
        assert!(matches!(renamed, ResolvedValue::Object(_)));

        let parent = ResolvedObject::new("User", User);
        let scalar = resolve(&resolvers(), "User", "updatedAt", Some(&parent), vec![]);
        assert!(matches!(scalar, ResolvedValue::Leaf(Value::String(_))));
    }
}
