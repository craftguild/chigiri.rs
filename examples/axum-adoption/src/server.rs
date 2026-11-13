//! Builds one Axum route from the generated schema and adapter.

use axum::Router;
use axum::routing::any;

use crate::application::AppContext;
use crate::domain::Types;
use crate::graphql::generated;
use crate::hooks::{ApplicationContextFactory, CookieResponseHook, RequestPolicy};
use crate::resolvers::{ItemFields, MutationFields, QueryFields, ViewerFields};

pub type Endpoint = chigiri::Endpoint<AppContext>;

pub fn endpoint() -> Endpoint {
    let resolvers = generated::Resolvers::<AppContext, Types>::builder(
        std::convert::identity::<chigiri::FieldError>,
    )
    .with_viewer(ViewerFields)
    .with_item(ItemFields)
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .build();
    chigiri::Endpoint::builder(resolvers)
        .inject(|components| {
            components.http().push_transport_admission(RequestPolicy);
        })
        .context_factory(ApplicationContextFactory)
        .response_hook(CookieResponseHook)
        .build()
        .expect("checked-in generated endpoint can be activated.")
}

pub fn router() -> Router {
    Router::new().route("/graphql", any(chigiri_axum::handler(endpoint())))
}
