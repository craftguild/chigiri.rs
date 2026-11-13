//! Connects Hyper `Incoming` bodies to the transport-neutral body SPI.

use chigiri_http::{BodyReadError, BoxFuture, RequestBody};
use http_body_util::BodyExt as _;
use hyper::body::Incoming;

pub(crate) struct HyperBody(pub(crate) Incoming);

impl RequestBody for HyperBody {
    type Chunk = bytes::Bytes;

    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        Box::pin(async move {
            while let Some(frame) = self.0.frame().await {
                let frame = frame.map_err(BodyReadError::new)?;
                if let Ok(data) = frame.into_data() {
                    return Ok(Some(data));
                }
            }
            Ok(None)
        })
    }
}
