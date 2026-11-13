//! Synthetic implementation group for product-object resolvers.

use crate::generated;
use crate::query::TrackingResolver;
use crate::resolver_macros::product_methods;
use crate::types::Types;

impl generated::ProductObject000Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum000, generated::Enum000::Alpha);
}

impl ::generated::ProductObject001Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
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

    async fn related<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _parent: &'a crate::types::Product,
        _args: generated::ProductObject001RelatedArgs,
    ) -> chigiri::FieldResult<Option<crate::types::Product>> {
        Ok(None)
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
}

impl generated::ProductObject002Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum002, generated::Enum002::Alpha);
}

impl generated::ProductObject003Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum003, generated::Enum003::Alpha);
}

impl generated::ProductObject004Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum004, generated::Enum004::Alpha);
}

impl generated::ProductObject005Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum005, generated::Enum005::Alpha);
}

impl generated::ProductObject006Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum006, generated::Enum006::Alpha);
}

impl generated::ProductObject007Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum007, generated::Enum007::Alpha);
}

impl generated::ProductObject008Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum008, generated::Enum008::Alpha);
}

impl generated::ProductObject009Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum009, generated::Enum009::Alpha);
}

impl generated::ProductObject010Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum010, generated::Enum010::Alpha);
}

impl generated::ProductObject011Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum011, generated::Enum011::Alpha);
}

impl generated::ProductObject012Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum012, generated::Enum012::Alpha);
}

impl generated::ProductObject013Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum013, generated::Enum013::Alpha);
}

impl generated::ProductObject014Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum014, generated::Enum014::Alpha);
}

impl generated::ProductObject015Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum015, generated::Enum015::Alpha);
}

impl generated::ProductObject016Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum016, generated::Enum016::Alpha);
}

impl generated::ProductObject017Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum017, generated::Enum017::Alpha);
}

impl generated::ProductObject018Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum018, generated::Enum018::Alpha);
}

impl generated::ProductObject019Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum019, generated::Enum019::Alpha);
}

impl generated::ProductObject020Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum020, generated::Enum020::Alpha);
}

impl generated::ProductObject021Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum021, generated::Enum021::Alpha);
}

impl generated::ProductObject022Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum022, generated::Enum022::Alpha);
}

impl generated::ProductObject023Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum023, generated::Enum023::Alpha);
}

impl generated::ProductObject024Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum024, generated::Enum024::Alpha);
}

impl generated::ProductObject025Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum025, generated::Enum025::Alpha);
}

impl generated::ProductObject026Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum026, generated::Enum026::Alpha);
}

impl generated::ProductObject027Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum027, generated::Enum027::Alpha);
}

impl generated::ProductObject028Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum028, generated::Enum028::Alpha);
}

impl generated::ProductObject029Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum029, generated::Enum029::Alpha);
}

impl generated::ProductObject030Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum030, generated::Enum030::Alpha);
}

impl generated::ProductObject031Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum031, generated::Enum031::Alpha);
}

impl generated::ProductObject032Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum032, generated::Enum032::Alpha);
}

impl generated::ProductObject033Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum033, generated::Enum033::Alpha);
}

impl generated::ProductObject034Resolver<(), Types, chigiri::FieldError> for TrackingResolver {
    product_methods!(generated::Enum034, generated::Enum034::Alpha);
}
