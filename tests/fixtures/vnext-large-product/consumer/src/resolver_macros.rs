//! Shared synthetic resolver implementations for symmetric field groups.

macro_rules! query_methods {
    ($(($method:ident, $args:ident)),+ $(,)?) => {
        $(
            async fn $method<'a>(
                &'a self,
                _ctx: chigiri::FieldContext<'a, ()>,
                _args: crate::generated::$args<crate::types::Types>,
            ) -> chigiri::FieldResult<crate::types::Product> {
                Ok(crate::types::Product)
            }
        )+
    };
}

macro_rules! product_methods {
    ($enum_type:ty, $enum_value:path) => {
        async fn id<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<chigiri::Id> {
            Ok("synthetic".into())
        }

        async fn name<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<String> {
            Ok("synthetic".to_owned())
        }

        async fn enabled<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<bool> {
            Ok(true)
        }

        async fn score<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<i32> {
            Ok(1)
        }

        async fn kind<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<crate::types::DomainKind> {
            Ok(crate::types::DomainKind::Alpha)
        }

        async fn custom<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<Option<crate::types::ScalarValue>> {
            Ok(None)
        }

        async fn next<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<Option<crate::types::Product>> {
            Ok(None)
        }

        async fn values<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<Vec<String>> {
            Ok(Vec::new())
        }

        async fn count<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<i32> {
            Ok(0)
        }

        async fn ratio<'a>(
            &'a self,
            _ctx: chigiri::FieldContext<'a, ()>,
            _parent: &'a crate::types::Product,
        ) -> chigiri::FieldResult<Option<f64>> {
            Ok(None)
        }
    };
}

pub(crate) use product_methods;
pub(crate) use query_methods;
