//! Bounded JSON response encoding with shared byte and node limits.

use std::io::{self, Write};

use crate::JsonConversionError;

pub(crate) struct BoundedWriter {
    bytes: Vec<u8>,
    maximum: usize,
}

impl BoundedWriter {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(maximum.min(8 * 1024)),
            maximum,
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn position(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn byte(&mut self, byte: u8) -> Result<(), JsonConversionError> {
        self.write_all(&[byte]).map_err(limit_error)
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), JsonConversionError> {
        self.write_all(bytes).map_err(limit_error)
    }

    pub(crate) fn formatted(
        &mut self,
        arguments: std::fmt::Arguments<'_>,
    ) -> Result<(), JsonConversionError> {
        self.write_fmt(arguments).map_err(limit_error)
    }

    pub(crate) fn string(&mut self, value: &str) -> Result<(), JsonConversionError> {
        serde_json::to_writer(self, value).map_err(|_| JsonConversionError::ResponseSizeLimit)
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.maximum.saturating_sub(self.bytes.len());
        if buffer.len() > remaining {
            return Err(io::Error::other("response size limit exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn limit_error(_: io::Error) -> JsonConversionError {
    JsonConversionError::ResponseSizeLimit
}
