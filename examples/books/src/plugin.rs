//! Adds sample response metadata in request middleware.

use std::pin::Pin;

use chigiri::{GraphQlResponse, Plugin, RequestContext, RequestNext};

use crate::server::AppContext;

pub(crate) struct RequestCountPlugin;

impl Plugin<AppContext> for RequestCountPlugin {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, AppContext>,
        next: RequestNext<'a, AppContext>,
    ) -> Pin<Box<dyn Future<Output = GraphQlResponse> + Send + 'a>> {
        Box::pin(async move {
            let count = context.context().catalog.observe_request();
            let mut response = next.run(context).await;
            response.extensions.insert("requestCount", count);
            response
        })
    }
}
