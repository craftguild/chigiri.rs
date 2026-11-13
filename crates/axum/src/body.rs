//! Axum `Body` integration for the transport-neutral body SPI.

use axum::body::Body;
use chigiri_http::{BodyReadError, BoxFuture, RequestBody};
use http_body_util::BodyExt as _;

pub(crate) struct AxumBody(pub(crate) Body);

impl RequestBody for AxumBody {
    type Chunk = axum::body::Bytes;

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
