//! Transport-independent request and response media-type handling.

use crate::{CodecError, RequestHeader};

/// GraphQL response media types supported by the codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseMediaType {
    /// The GraphQL response media type.
    GraphQlResponseJson,
    /// Legacy JSON media type.
    Json,
}

impl ResponseMediaType {
    /// Returns the UTF-8 value used for `Content-Type`.
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::GraphQlResponseJson => "application/graphql-response+json; charset=utf-8",
            Self::Json => "application/json; charset=utf-8",
        }
    }
}

/// Selects a response media type from the `Accept` header values by quality and
/// server preference.
pub fn negotiate_response_media(
    headers: &[RequestHeader<'_>],
) -> Result<ResponseMediaType, CodecError> {
    let values = header_values(headers, "accept");
    if values.is_empty() {
        return Ok(ResponseMediaType::GraphQlResponseJson);
    }
    let mut modern = Match::default();
    let mut legacy = Match::default();
    for value in values {
        let Ok(value) = std::str::from_utf8(value) else {
            continue;
        };
        for range in value.split(',') {
            let Some(range) = parse_range(range) else {
                continue;
            };
            let precedence = (range.kind_specificity(), range.parameter_count);
            if range
                .kind
                .eq_ignore_ascii_case("application/graphql-response+json")
            {
                modern.observe(precedence, range.quality);
            } else if range.kind.eq_ignore_ascii_case("application/json") {
                legacy.observe(precedence, range.quality);
            } else if range.kind.eq_ignore_ascii_case("application/*") || range.kind == "*/*" {
                modern.observe(precedence, range.quality);
                legacy.observe(precedence, range.quality);
            }
        }
    }
    match (modern.quality(), legacy.quality()) {
        (0, 0) => Err(CodecError::not_acceptable()),
        (modern, legacy) if modern >= legacy => Ok(ResponseMediaType::GraphQlResponseJson),
        _ => Ok(ResponseMediaType::Json),
    }
}

#[derive(Clone, Copy, Default)]
struct Match {
    precedence: Option<(u8, u8)>,
    quality: u16,
}

impl Match {
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

pub(crate) fn accepts_json_content_type(headers: &[RequestHeader<'_>]) -> bool {
    let values = header_values(headers, "content-type");
    if values.len() != 1 {
        return false;
    }
    let Ok(value) = std::str::from_utf8(values[0]) else {
        return false;
    };
    let mut segments = value.split(';');
    if !segments
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
    {
        return false;
    }
    segments.all(|parameter| {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case("charset")
            && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
    })
}

fn header_values<'a>(headers: &'a [RequestHeader<'a>], name: &str) -> Vec<&'a [u8]> {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value)
        .collect()
}

struct ParsedRange<'a> {
    kind: &'a str,
    parameter_count: u8,
    quality: u16,
}

impl ParsedRange<'_> {
    fn kind_specificity(&self) -> u8 {
        match self.kind {
            "*/*" => 0,
            kind if kind.ends_with("/*") => 1,
            _ => 2,
        }
    }
}

fn parse_range(range: &str) -> Option<ParsedRange<'_>> {
    let mut segments = range.trim().split(';');
    let kind = segments.next()?.trim();
    let mut quality = 1_000;
    let mut parameter_count = 0;
    let mut saw_quality = false;
    for parameter in segments {
        let (name, value) = parameter.trim().split_once('=')?;
        if name.trim().eq_ignore_ascii_case("q") {
            if saw_quality {
                return None;
            }
            quality = parse_quality(value.trim())?;
            saw_quality = true;
        } else if !saw_quality
            && name.trim().eq_ignore_ascii_case("charset")
            && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
            && parameter_count == 0
        {
            parameter_count = 1;
        } else {
            // A range with parameters absent from the offered representation does not match.
            return None;
        }
    }
    Some(ParsedRange {
        kind,
        parameter_count,
        quality,
    })
}

fn parse_quality(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let fraction = format!("{fraction:0<3}").parse::<u16>().ok()?;
    match whole {
        "0" => Some(fraction),
        "1" if fraction == 0 => Some(1_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponseMediaType, negotiate_response_media};
    use crate::RequestHeader;

    #[test]
    fn interprets_quality_and_wildcards_and_prefers_the_dedicated_media_on_ties() {
        let headers = [RequestHeader::new(
            "Accept",
            b"application/json;q=0.8, application/graphql-response+json;q=0.8",
        )];
        assert_eq!(
            negotiate_response_media(&headers).unwrap(),
            ResponseMediaType::GraphQlResponseJson
        );
        let headers = [RequestHeader::new("accept", b"application/*;q=0.4")];
        assert_eq!(
            negotiate_response_media(&headers).unwrap(),
            ResponseMediaType::GraphQlResponseJson
        );
    }

    #[test]
    fn rejects_zero_or_malformed_quality_values() {
        let headers = [RequestHeader::new(
            "accept",
            b"application/json;q=0, text/plain",
        )];
        assert_eq!(
            negotiate_response_media(&headers).unwrap_err().status(),
            406
        );
    }

    #[test]
    fn exact_zero_quality_is_more_important_than_wildcard() {
        let headers = [RequestHeader::new(
            "accept",
            b"application/graphql-response+json;q=0, application/json;q=0.5, */*;q=1",
        )];
        assert_eq!(
            negotiate_response_media(&headers).unwrap(),
            ResponseMediaType::Json
        );
    }

    #[test]
    fn media_parameter_match_and_specificity_precede_quality() {
        let headers = [RequestHeader::new(
            "accept",
            b"application/graphql-response+json;q=0.5, application/graphql-response+json;charset=utf-8;q=0.1, application/json;q=0.3",
        )];
        assert_eq!(
            negotiate_response_media(&headers).unwrap(),
            ResponseMediaType::Json
        );

        let headers = [RequestHeader::new(
            "accept",
            b"application/graphql-response+json;charset=iso-8859-1, application/json;q=0.5",
        )];
        assert_eq!(
            negotiate_response_media(&headers).unwrap(),
            ResponseMediaType::Json
        );
    }
}
