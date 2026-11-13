//! Standard policy for rejecting browser simple requests without a custom preflight header.

use std::fmt;

use chigiri_core::GraphQlError;

use crate::{
    AdmissionDecision, AdmissionError, BoxFuture, Rejection, TransportAdmission, TransportRequest,
};

const HEADER_NAME_BYTES: usize = 64;

/// CSRF/XS-Search policy requiring a non-simple header for GET requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsrfProtection {
    header_name: String,
}

impl CsrfProtection {
    /// Creates a policy from a validated custom header name.
    pub fn new(header_name: impl Into<String>) -> Result<Self, CsrfConfigurationError> {
        let header_name = header_name.into();
        if header_name.is_empty()
            || header_name.len() > HEADER_NAME_BYTES
            || !header_name.bytes().all(is_header_name_byte)
        {
            return Err(CsrfConfigurationError);
        }
        Ok(Self {
            header_name: header_name.to_ascii_lowercase(),
        })
    }

    /// Returns the production policy using `x-chigiri-preflight`.
    pub fn production() -> Self {
        Self {
            header_name: "x-chigiri-preflight".to_owned(),
        }
    }

    pub fn header_name(&self) -> &str {
        &self.header_name
    }
}

impl TransportAdmission for CsrfProtection {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(async move {
            if request.method() != "GET"
                || request.headers().iter().any(|header| {
                    header.name.eq_ignore_ascii_case(&self.header_name) && !header.value.is_empty()
                })
            {
                return Ok(AdmissionDecision::Continue);
            }
            let error =
                GraphQlError::new("Browser preflight header is required").code("CSRF_PREVENTED");
            let rejection = Rejection::new(403, error)
                .expect("The fixed status of CSRF rejection is 400 to 599.");
            Ok(AdmissionDecision::Reject(rejection))
        })
    }
}

/// The format or byte limit of the custom preflight header name is violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CsrfConfigurationError;

impl fmt::Display for CsrfConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CSRF preflight header name is invalid")
    }
}

impl std::error::Error for CsrfConfigurationError {}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::CsrfProtection;
    use crate::{ApplicationMetadata, RequestHeader, TransportAdmission, TransportRequest};

    #[test]
    fn detects_simple_requests() {
        let policy = CsrfProtection::production();
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("x-chigiri-preflight", b"1")];
        let get =
            |headers| TransportRequest::new("GET", "/graphql", None, headers, None, &metadata);
        let accepted = futures_lite(&policy, get(&headers));
        let rejected = futures_lite(&policy, get(&[]));
        assert!(matches!(accepted, crate::AdmissionDecision::Continue));
        assert!(matches!(rejected, crate::AdmissionDecision::Reject(_)));
    }

    fn futures_lite(
        policy: &CsrfProtection,
        request: TransportRequest<'_>,
    ) -> crate::AdmissionDecision {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};

        let mut future = Box::pin(policy.admit(request));
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(Ok(decision)) => decision,
            _ => panic!("CSRF policy needs to be completed in time."),
        }
    }
}
