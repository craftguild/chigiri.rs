//! Complete ordinary traits for a request-local context and shared domain services.

use chigiri::FieldContext;
use domain::{AppError, User};

use crate::Types;
use crate::context::AppContext;

pub(crate) struct Fields;

impl generated::QueryResolver<AppContext, Types, AppError> for Fields {
    async fn user<'a>(
        &'a self,
        ctx: FieldContext<'a, AppContext>,
        args: generated::QueryUserArgs,
    ) -> Result<Option<User>, AppError> {
        Ok(Some(ctx.context().users.find(args.id.as_str()).await?))
    }
    async fn users<'a>(&'a self, ctx: FieldContext<'a, AppContext>) -> Result<Vec<User>, AppError> {
        Ok(vec![ctx.context().users.find("ada").await?])
    }
    async fn internal<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
    ) -> Result<Option<String>, AppError> {
        Err(std::io::Error::other("private HTTP credential").into())
    }
    async fn required<'a>(&'a self, _: FieldContext<'a, AppContext>) -> Result<String, AppError> {
        Err(AppError::NotFound)
    }
    async fn success<'a>(&'a self, ctx: FieldContext<'a, AppContext>) -> Result<String, AppError> {
        let context = ctx.context();
        let mut cache = context.cache.lock().unwrap();
        cache.push("visited".to_owned());
        Ok(format!(
            "{}:{}:{}",
            context.session,
            context.id,
            cache.len()
        ))
    }
    async fn floats<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
    ) -> Result<Vec<Option<f64>>, AppError> {
        Ok(vec![Some(f64::NAN)])
    }
}

impl generated::MutationResolver<AppContext, Types, AppError> for Fields {
    async fn rename<'a>(
        &'a self,
        ctx: FieldContext<'a, AppContext>,
        args: generated::MutationRenameArgs,
    ) -> Result<User, AppError> {
        let mut user = ctx.context().users.find(args.id.as_str()).await?;
        user.name = args.name;
        Ok(user)
    }
}

impl generated::UserResolver<AppContext, Types, AppError> for Fields {
    async fn name<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        user: &'a User,
    ) -> Result<String, AppError> {
        Ok(user.name.clone())
    }
    async fn display_name<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        user: &'a User,
    ) -> Result<String, AppError> {
        Ok(format!("{} {}", user.surname, user.given_name))
    }
    async fn failure<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        _: &'a User,
    ) -> Result<Option<String>, AppError> {
        Err(AppError::NotFound)
    }
}
