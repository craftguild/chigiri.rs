//! Passive accounting for bytes accepted by a WebSocket writer.

use std::fmt;
use std::io::IoSlice;

mod parser;

use parser::{FrameKind, FrameParser};

/// A failure to correlate accepted RFC 6455 bytes with one armed text frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketWriteProgressError {
    /// A previous observation already failed closed.
    Failed,
    /// A second application text frame was armed before the first was flushed.
    AlreadyArmed,
    /// An accepted text frame was not the one currently armed by the adapter.
    UnexpectedText,
    /// The accepted frame header was malformed or unsupported.
    MalformedFrame,
    /// The accepted text frame did not have the armed payload length.
    TextLengthMismatch,
    /// A caller-reported vectored prefix exceeded the supplied buffers.
    AcceptedPrefixOutOfBounds,
    /// A checked length or progress calculation overflowed.
    LengthOverflow,
    /// Flush completed before the armed frame and any following control finished.
    IncompleteFlush,
}

impl fmt::Display for WebSocketWriteProgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Failed => "WebSocket write observation has failed closed",
            Self::AlreadyArmed => "a WebSocket text write is already armed",
            Self::UnexpectedText => "an unexpected WebSocket text frame was accepted",
            Self::MalformedFrame => "a malformed WebSocket frame was accepted",
            Self::TextLengthMismatch => "the WebSocket text payload length did not match",
            Self::AcceptedPrefixOutOfBounds => {
                "the accepted WebSocket prefix exceeded the supplied buffers"
            }
            Self::LengthOverflow => "WebSocket write progress overflowed",
            Self::IncompleteFlush => "the armed WebSocket text frame was not fully flushed",
        })
    }
}

impl std::error::Error for WebSocketWriteProgressError {}

/// Progress derived from one actually accepted write prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSocketAcceptedTextProgress {
    application_bytes: usize,
    payload_complete: bool,
}

impl WebSocketAcceptedTextProgress {
    /// Returns newly accepted application text payload bytes.
    pub const fn application_bytes(self) -> usize {
        self.application_bytes
    }

    /// Reports whether the currently armed text payload is complete.
    ///
    /// Completion is distinct from a successful upstream flush.
    pub const fn payload_complete(self) -> bool {
        self.payload_complete
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArmedText {
    Idle,
    Waiting { expected: u64, accepted: u64 },
    Complete,
}

/// Passively observes server WebSocket bytes accepted by an underlying writer.
///
/// This fixed-storage scanner is not an RFC 6455 writer. An adapter arms one
/// immutable application text frame before calling its upstream `start_send`,
/// then supplies only the prefixes that `AsyncWrite` actually accepted. Frame
/// headers and control payloads are validated but never reported as application
/// progress. The adapter, not this tracker, retains the physical frame owner
/// until the upstream flush and the owner's final drop.
#[derive(Debug)]
pub struct WebSocketWriteProgressTracker {
    parser: FrameParser,
    armed: ArmedText,
    failed: bool,
}

impl WebSocketWriteProgressTracker {
    /// Creates an idle passive tracker.
    pub const fn new() -> Self {
        Self {
            parser: FrameParser::new(),
            armed: ArmedText::Idle,
            failed: false,
        }
    }

    /// Arms exactly one expected application text payload.
    ///
    /// A partially accepted control frame may straddle this call. A text frame
    /// whose header already began before arming is rejected.
    pub fn arm_text(
        &mut self,
        expected_payload_bytes: usize,
    ) -> Result<(), WebSocketWriteProgressError> {
        self.ensure_live()?;
        if self.armed != ArmedText::Idle {
            return self.fail(WebSocketWriteProgressError::AlreadyArmed);
        }
        if self.parser.partial_text_header() {
            return self.fail(WebSocketWriteProgressError::UnexpectedText);
        }
        let expected = u64::try_from(expected_payload_bytes)
            .map_err(|_| WebSocketWriteProgressError::LengthOverflow)
            .or_else(|error| self.fail(error))?;
        if expected > i64::MAX as u64 {
            return self.fail(WebSocketWriteProgressError::LengthOverflow);
        }
        self.armed = ArmedText::Waiting {
            expected,
            accepted: 0,
        };
        Ok(())
    }

    /// Observes one contiguous prefix accepted by the underlying writer.
    pub fn observe_accepted(
        &mut self,
        accepted: &[u8],
    ) -> Result<WebSocketAcceptedTextProgress, WebSocketWriteProgressError> {
        self.ensure_live()?;
        let application_bytes = match self.observe_slice(accepted) {
            Ok(application_bytes) => application_bytes,
            Err(error) => return self.fail(error),
        };
        Ok(self.progress(application_bytes))
    }

    /// Observes `accepted_prefix` bytes from a vectored underlying write.
    pub fn observe_accepted_vectored(
        &mut self,
        buffers: &[IoSlice<'_>],
        accepted_prefix: usize,
    ) -> Result<WebSocketAcceptedTextProgress, WebSocketWriteProgressError> {
        self.ensure_live()?;
        let available = buffers
            .iter()
            .try_fold(0usize, |total, buffer| total.checked_add(buffer.len()));
        if available.is_none() {
            return self.fail(WebSocketWriteProgressError::LengthOverflow);
        }
        if accepted_prefix > available.expect("checked above") {
            return self.fail(WebSocketWriteProgressError::AcceptedPrefixOutOfBounds);
        }

        let mut remaining = accepted_prefix;
        let mut application_bytes = 0usize;
        for buffer in buffers {
            let take = remaining.min(buffer.len());
            let delta = match self.observe_slice(&buffer[..take]) {
                Ok(delta) => delta,
                Err(error) => return self.fail(error),
            };
            application_bytes = application_bytes
                .checked_add(delta)
                .ok_or(WebSocketWriteProgressError::LengthOverflow)
                .or_else(|error| self.fail(error))?;
            remaining -= take;
            if remaining == 0 {
                break;
            }
        }
        Ok(self.progress(application_bytes))
    }

    /// Confirms upstream flush completion and releases the armed text slot.
    ///
    /// Payload completion alone does not call this method or release the slot.
    pub fn finish_flush(&mut self) -> Result<(), WebSocketWriteProgressError> {
        self.ensure_live()?;
        if self.armed != ArmedText::Complete || !self.parser.at_frame_boundary() {
            return self.fail(WebSocketWriteProgressError::IncompleteFlush);
        }
        self.armed = ArmedText::Idle;
        Ok(())
    }

    fn observe_slice(&mut self, mut bytes: &[u8]) -> Result<usize, WebSocketWriteProgressError> {
        let mut application_bytes = 0usize;
        while !bytes.is_empty() {
            if self.parser.at_header() {
                let consumed = self.parser.accept_header(bytes);
                bytes = &bytes[consumed..];
                let Some((kind, payload_len)) = self.parser.decode_header()? else {
                    continue;
                };
                self.begin_frame(kind, payload_len)?;
                continue;
            }

            let (kind, accepted, complete) = self.parser.accept_payload(bytes);
            bytes = &bytes[accepted..];
            if kind == FrameKind::Text {
                application_bytes = application_bytes
                    .checked_add(accepted)
                    .ok_or(WebSocketWriteProgressError::LengthOverflow)
                    .or_else(|error| self.fail(error))?;
                self.accept_text(accepted)?;
            }
            if complete {
                self.parser.finish_payload();
                if kind == FrameKind::Text {
                    self.complete_text()?;
                }
            }
        }
        Ok(application_bytes)
    }

    fn begin_frame(
        &mut self,
        kind: FrameKind,
        payload_len: u64,
    ) -> Result<(), WebSocketWriteProgressError> {
        if kind == FrameKind::Text {
            let ArmedText::Waiting { expected, .. } = self.armed else {
                return self.fail(WebSocketWriteProgressError::UnexpectedText);
            };
            if payload_len != expected {
                return self.fail(WebSocketWriteProgressError::TextLengthMismatch);
            }
        }
        self.parser.begin_payload(kind, payload_len);
        if payload_len == 0 {
            self.parser.finish_payload();
            if kind == FrameKind::Text {
                self.complete_text()?;
            }
        }
        Ok(())
    }

    fn accept_text(&mut self, accepted: usize) -> Result<(), WebSocketWriteProgressError> {
        let ArmedText::Waiting {
            expected,
            accepted: previous,
        } = self.armed
        else {
            return self.fail(WebSocketWriteProgressError::UnexpectedText);
        };
        let accepted = u64::try_from(accepted)
            .ok()
            .and_then(|accepted| previous.checked_add(accepted))
            .ok_or(WebSocketWriteProgressError::LengthOverflow)
            .or_else(|error| self.fail(error))?;
        if accepted > expected {
            return self.fail(WebSocketWriteProgressError::TextLengthMismatch);
        }
        self.armed = ArmedText::Waiting { expected, accepted };
        Ok(())
    }

    fn complete_text(&mut self) -> Result<(), WebSocketWriteProgressError> {
        let ArmedText::Waiting { expected, accepted } = self.armed else {
            return self.fail(WebSocketWriteProgressError::UnexpectedText);
        };
        if accepted != expected {
            return self.fail(WebSocketWriteProgressError::TextLengthMismatch);
        }
        self.armed = ArmedText::Complete;
        Ok(())
    }

    const fn progress(&self, application_bytes: usize) -> WebSocketAcceptedTextProgress {
        WebSocketAcceptedTextProgress {
            application_bytes,
            payload_complete: matches!(self.armed, ArmedText::Complete),
        }
    }

    fn ensure_live(&self) -> Result<(), WebSocketWriteProgressError> {
        if self.failed {
            Err(WebSocketWriteProgressError::Failed)
        } else {
            Ok(())
        }
    }

    fn fail<T>(
        &mut self,
        error: WebSocketWriteProgressError,
    ) -> Result<T, WebSocketWriteProgressError> {
        self.failed = true;
        Err(error)
    }
}

impl Default for WebSocketWriteProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}
