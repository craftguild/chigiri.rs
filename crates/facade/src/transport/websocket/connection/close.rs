//! Fixed safe WebSocket terminal descriptions.

use std::fmt;

/// One protocol close code and its fixed credential-free reason.
#[derive(Clone, Copy, Eq, PartialEq)]
#[doc(hidden)]
pub struct WebSocketConnectionClose {
    code: u16,
    reason: &'static str,
}

impl WebSocketConnectionClose {
    pub(super) const BAD_REQUEST: Self = Self::new(4400, "Bad Request");
    pub(super) const UNAUTHORIZED: Self = Self::new(4401, "Unauthorized");
    pub(super) const FORBIDDEN: Self = Self::new(4403, "Forbidden");
    pub(super) const TIMED_OUT: Self = Self::new(4408, "Connection initialisation timeout");
    pub(super) const TOO_MANY_INIT: Self = Self::new(4429, "Too many initialisation requests");
    pub(super) const INTERNAL: Self = Self::new(1011, "Internal server error");

    pub(crate) const fn bad_request() -> Self {
        Self::BAD_REQUEST
    }

    pub(crate) const fn internal() -> Self {
        Self::INTERNAL
    }

    pub(crate) const fn duplicate_operation() -> Self {
        Self::new(4409, "Subscriber already exists")
    }

    const fn new(code: u16, reason: &'static str) -> Self {
        Self { code, reason }
    }

    /// Returns the frozen protocol close code.
    pub const fn code(self) -> u16 {
        self.code
    }

    /// Returns the fixed safe reason, never an application error.
    pub const fn reason(self) -> &'static str {
        self.reason
    }
}

impl fmt::Debug for WebSocketConnectionClose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketConnectionClose")
            .field("code", &self.code)
            .field("reason", &self.reason)
            .finish()
    }
}
