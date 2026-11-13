#![deny(warnings)]
#![cfg(test)]

pub use ::generated;

#[cfg(test)]
mod consumer_tests;

use chigiri::FieldResult;

struct Types;

#[derive(Clone, Copy)]
enum DomainKind {
    Alpha,
    RejectOutput,
}

impl generated::SchemaTypes for Types {
    type ProtocolKind = DomainKind;

    fn protocol_kind_from_graphql(
        value: generated::ProtocolKind,
    ) -> Result<Self::ProtocolKind, chigiri::EnumBindingError> {
        match value {
            generated::ProtocolKind::Alpha => Ok(DomainKind::Alpha),
            generated::ProtocolKind::Beta => {
                Err(chigiri::EnumBindingError::new(SecretError("input secret")))
            }
        }
    }

    fn protocol_kind_to_graphql(
        value: &Self::ProtocolKind,
    ) -> Result<generated::ProtocolKind, chigiri::EnumBindingError> {
        match value {
            DomainKind::Alpha => Ok(generated::ProtocolKind::Alpha),
            DomainKind::RejectOutput => {
                Err(chigiri::EnumBindingError::new(SecretError("output secret")))
            }
        }
    }
}

struct Query;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn direct<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryDirectArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(args.value)
    }

    async fn defaulted<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryDefaultedArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(args.value)
    }

    async fn list<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryListArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(args
            .values
            .into_iter()
            .next()
            .expect("expected a non-null list item."))
    }

    async fn nullable_list<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryNullableListArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(args
            .values
            .into_iter()
            .flatten()
            .next()
            .expect("expected one item in the non-null list."))
    }

    async fn list_output<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> FieldResult<Vec<DomainKind>> {
        Ok(vec![DomainKind::Alpha, DomainKind::Alpha])
    }

    async fn list_failure<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> FieldResult<Vec<DomainKind>> {
        Ok(vec![DomainKind::Alpha, DomainKind::RejectOutput])
    }

    async fn nullable_list_failure<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> FieldResult<Vec<Option<DomainKind>>> {
        Ok(vec![
            Some(DomainKind::Alpha),
            Some(DomainKind::RejectOutput),
        ])
    }

    async fn object<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryObjectArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(args.input.nested.kind)
    }

    async fn recursive<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryRecursiveArgs<Types>,
    ) -> FieldResult<DomainKind> {
        Ok(deepest_kind(args.input))
    }

    async fn choice<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryChoiceArgs<Types>,
    ) -> FieldResult<DomainKind> {
        match args.input {
            generated::ChoiceInput::Kind(value) => Ok(value),
            generated::ChoiceInput::Nested(value) => Ok(value.kind),
        }
    }

    async fn nested_choice<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryNestedChoiceArgs<Types>,
    ) -> FieldResult<DomainKind> {
        match args.input.choice {
            generated::ChoiceInput::Kind(value) => Ok(value),
            generated::ChoiceInput::Nested(value) => Ok(value.kind),
        }
    }

    async fn output<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> FieldResult<Option<DomainKind>> {
        Ok(Some(DomainKind::RejectOutput))
    }

    async fn owned<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: generated::QueryOwnedArgs,
    ) -> FieldResult<generated::OwnedKind> {
        Ok(args.value)
    }
}

fn deepest_kind(input: generated::RecursiveInput<Types>) -> DomainKind {
    match input.next {
        chigiri::Input::Value(next) => deepest_kind(*next),
        chigiri::Input::Undefined | chigiri::Input::Null => input.kind,
    }
}

#[derive(Debug)]
struct SecretError(&'static str);

impl std::fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SecretError {}
