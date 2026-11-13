//! Runtime-neutral chunk-stream abstraction over framework request bodies.

use std::error::Error;
use std::fmt;

use crate::BoxFuture;

/// Request-body failure whose source is not exposed to the client.
pub struct BodyReadError(Box<dyn Error + Send + Sync>);

impl BodyReadError {
    /// Retains a framework-specific error as the internal source.
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for BodyReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BodyReadError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for BodyReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HTTP request body could not be read")
    }
}

impl Error for BodyReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Runtime-neutral SPI that yields request-body chunks.
pub trait RequestBody: Send {
    type Chunk: AsRef<[u8]> + Send;

    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>>;
}

/// Convenience body type for already-buffered bytes.
pub struct BufferedBody(Option<Vec<u8>>);

impl BufferedBody {
    /// Creates a body from owned bytes.
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(Some(bytes))
    }

    /// Creates an empty body.
    pub const fn empty() -> Self {
        Self::new(Vec::new())
    }
}

impl RequestBody for BufferedBody {
    type Chunk = Vec<u8>;

    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        let chunk = self.0.take();
        Box::pin(async move { Ok(chunk) })
    }
}
