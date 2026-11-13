//! Negotiation for an already selected Subscription, not finite HTTP dispatch.

mod content_type;
mod range;
mod syntax;

use super::SubscriptionTransport;
use crate::{CodecError, RequestHeader, ResponseMediaType};

use range::MediaRange;
use syntax::Pieces;

/// Checks the frozen HTTP streaming profile without reading a request body.
///
/// Callers select a Subscription separately: a wildcard here must never turn a
/// normal finite Query or Mutation into a streaming response. Missing Accept is
/// rejected because these profiles require an admitting range. Multipart also
/// requires a positive matching subscriptionSpec=1.0 range. Exact ranges (and
/// then parameter counts) take precedence over wildcards before quality; ties
/// between the two representations prefer SSE. No input-sized collection is
/// allocated while scanning header lines, ranges, or parameters.
pub(crate) fn negotiate(
    method: &str,
    headers: &[RequestHeader<'_>],
) -> Result<SubscriptionTransport, CodecError> {
    let transport = select(headers)?;
    if method != "POST" {
        return Err(CodecError::streaming_method_not_allowed());
    }
    if !content_type::accepts_json(headers) {
        return Err(CodecError::unsupported_media(
            ResponseMediaType::GraphQlResponseJson,
        ));
    }
    Ok(transport)
}

fn select(headers: &[RequestHeader<'_>]) -> Result<SubscriptionTransport, CodecError> {
    let mut sse = MediaMatch::default();
    let mut multipart = MediaMatch::default();
    let mut multipart_opt_in = false;
    for header in headers {
        if !header.name.eq_ignore_ascii_case("accept") {
            continue;
        }
        let Ok(value) = std::str::from_utf8(header.value) else {
            continue;
        };
        for piece in Pieces::new(value, b',') {
            let Some(range) = MediaRange::parse(piece) else {
                continue;
            };
            if let Some(precedence) = range.sse_precedence() {
                sse.observe(precedence, range.quality);
            }
            if let Some(precedence) = range.multipart_precedence() {
                multipart.observe(precedence, range.quality);
                multipart_opt_in |= range.opts_into_multipart();
            }
        }
    }
    let sse = sse.quality();
    let multipart = if multipart_opt_in {
        multipart.quality()
    } else {
        0
    };
    match (sse, multipart) {
        (0, 0) => Err(CodecError::not_acceptable()),
        (sse, multipart) if sse >= multipart => Ok(SubscriptionTransport::ServerSentEvents),
        _ => Ok(SubscriptionTransport::Multipart),
    }
}

#[derive(Default)]
struct MediaMatch {
    precedence: Option<(u8, u8)>,
    quality: u16,
}

impl MediaMatch {
    fn observe(&mut self, precedence: (u8, u8), quality: u16) {
        match self.precedence {
            Some(current) if current > precedence => {}
            Some(current) if current == precedence => self.quality = self.quality.max(quality),
            _ => {
                self.precedence = Some(precedence);
                self.quality = quality;
            }
        }
    }

    fn quality(self) -> u16 {
        self.precedence.map_or(0, |_| self.quality)
    }
}
