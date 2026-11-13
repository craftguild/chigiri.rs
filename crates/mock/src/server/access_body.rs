//! Preserve the response status and record body completion or drop exactly once.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::response::Response;
use chigiri_mock::logging::Access;
use http_body::{Body as HttpBody, Frame, SizeHint};

pub(super) fn attach(mut response: Response, mut access: Access, head: bool) -> Response {
    access.response(response.status().as_u16());
    if head || response.body().is_end_stream() {
        access.complete();
        return response;
    }
    let (parts, body) = response.into_parts();
    response = Response::from_parts(
        parts,
        Body::new(Tracked {
            body,
            access: Some(access),
        }),
    );
    response
}
struct Tracked {
    body: Body,
    access: Option<Access>,
}
impl HttpBody for Tracked {
    type Data = <Body as HttpBody>::Data;
    type Error = <Body as HttpBody>::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.body).poll_frame(context);
        match &result {
            Poll::Ready(Some(Err(_))) => {
                this.access.take();
            }
            Poll::Ready(None) => this.complete(),
            Poll::Ready(Some(Ok(_))) if this.body.is_end_stream() => this.complete(),
            _ => {}
        }
        result
    }
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}
impl Tracked {
    fn complete(&mut self) {
        if let Some(mut access) = self.access.take() {
            access.complete();
        }
    }
}
