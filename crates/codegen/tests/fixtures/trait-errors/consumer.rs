//! Ordinary API-layer methods retain external domain values and application errors.

use domain::{AppError, User};

pub struct Types;

impl generated::SchemaTypes for Types {
    type User = User;
}

pub struct AppContext {
    pub users: std::sync::Arc<domain::Users>,
}

pub struct QueryFields;

impl generated::QueryResolver<AppContext, Types, AppError> for QueryFields {
    async fn user<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        args: generated::QueryUserArgs,
    ) -> Result<Option<User>, AppError> {
        Ok(Some(ctx.context().users.find(args.id.as_str()).await?))
    }

    async fn users<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
    ) -> Result<Vec<User>, AppError> {
        Ok(vec![ctx.context().users.find("ada").await?])
    }

    async fn internal<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
    ) -> Result<Option<String>, AppError> {
        let result: Result<(), std::io::Error> =
            Err(std::io::Error::other("private resolver credential"));
        result?;
        Ok(None)
    }

    async fn required<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
    ) -> Result<String, AppError> {
        Err(AppError::NotFound)
    }

    async fn success<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
    ) -> Result<String, AppError> {
        Ok("ready".to_owned())
    }

    async fn floats<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
    ) -> Result<Vec<Option<f64>>, AppError> {
        Ok(vec![Some(1.0), Some(f64::NAN), None])
    }
}

pub struct MutationFields;

impl generated::MutationResolver<AppContext, Types, AppError> for MutationFields {
    async fn rename<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        args: generated::MutationRenameArgs,
    ) -> Result<User, AppError> {
        let mut user = ctx.context().users.find(args.id.as_str()).await?;
        user.name = args.name;
        Ok(user)
    }
}

pub struct UserFields;

impl generated::UserResolver<AppContext, Types, AppError> for UserFields {
    // Methods intentionally differ from SDL order: only group assembly is ordered.
    async fn display_name<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
        user: &'a User,
    ) -> Result<String, AppError> {
        Ok(format!("{} {}", user.surname, user.given_name))
    }

    async fn name<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
        user: &'a User,
    ) -> Result<String, AppError> {
        Ok(user.name.clone())
    }

    async fn failure<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
        _user: &'a User,
    ) -> Result<Option<String>, AppError> {
        Err(AppError::NotFound)
    }
}

pub fn map_app_error(error: AppError) -> chigiri::FieldError {
    match error {
        AppError::NotFound => chigiri::FieldError::public("User was not found").code("NOT_FOUND"),
        other => chigiri::FieldError::internal(other),
    }
}

pub fn resolvers<F>(mapper: F) -> generated::Resolvers<AppContext, Types>
where
    F: Fn(AppError) -> chigiri::FieldError + Send + Sync + 'static,
{
    generated::Resolvers::<AppContext, Types>::builder(mapper)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .with_user(UserFields)
        .build()
}
