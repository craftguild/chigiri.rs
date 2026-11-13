//! Matching one media range against the two fixed HTTP stream representations.

use super::syntax::{ParameterValue, Pieces, quality, token, trim};

pub(super) struct MediaRange<'a> {
    major: &'a str,
    minor: &'a str,
    charset: Option<ParameterValue<'a>>,
    subscription_spec: Option<ParameterValue<'a>>,
    boundary: Option<ParameterValue<'a>>,
    pub(super) quality: u16,
    parameter_count: u8,
}

impl<'a> MediaRange<'a> {
    pub(super) fn parse(input: &'a str) -> Option<Self> {
        let mut pieces = Pieces::new(trim(input), b';');
        let (major, minor) = trim(pieces.next()?).split_once('/')?;
        if !token(major) || !token(minor) || (major == "*" && minor != "*") {
            return None;
        }
        let mut parsed = Self {
            major,
            minor,
            charset: None,
            subscription_spec: None,
            boundary: None,
            quality: 1_000,
            parameter_count: 0,
        };
        let mut saw_quality = false;
        for piece in pieces {
            let (name, value) = trim(piece).split_once('=')?;
            let (name, value) = (trim(name), trim(value));
            if !token(name) {
                return None;
            }
            if name.eq_ignore_ascii_case("q") {
                if saw_quality {
                    return None;
                }
                // Recipients process q as weight at any parameter position.
                parsed.quality = quality(value)?;
                saw_quality = true;
                continue;
            }
            let value = ParameterValue::parse(value)?;
            let target = if name.eq_ignore_ascii_case("charset") {
                &mut parsed.charset
            } else if name.eq_ignore_ascii_case("subscriptionSpec") {
                &mut parsed.subscription_spec
            } else if name.eq_ignore_ascii_case("boundary") {
                &mut parsed.boundary
            } else {
                // Unknown representation parameters cannot match either fixed offer.
                return None;
            };
            if target.replace(value).is_some() {
                return None;
            }
            parsed.parameter_count += 1;
        }
        Some(parsed)
    }

    pub(super) fn sse_precedence(&self) -> Option<(u8, u8)> {
        if self.subscription_spec.is_some()
            || self.boundary.is_some()
            || self
                .charset
                .is_some_and(|charset| !charset.equals("utf-8", true))
        {
            return None;
        }
        self.precedence("text", "event-stream")
    }

    pub(super) fn multipart_precedence(&self) -> Option<(u8, u8)> {
        if self.charset.is_some()
            || self
                .subscription_spec
                .is_some_and(|version| !version.equals("1.0", false))
            || self
                .boundary
                .is_some_and(|boundary| !boundary.equals("graphql", false))
        {
            return None;
        }
        self.precedence("multipart", "mixed")
    }

    pub(super) fn opts_into_multipart(&self) -> bool {
        self.subscription_spec.is_some() && self.quality > 0
    }

    fn precedence(&self, major: &str, minor: &str) -> Option<(u8, u8)> {
        let specificity = if self.major == "*" && self.minor == "*" {
            0
        } else if self.major.eq_ignore_ascii_case(major) && self.minor == "*" {
            1
        } else if self.major.eq_ignore_ascii_case(major) && self.minor.eq_ignore_ascii_case(minor) {
            2
        } else {
            return None;
        };
        Some((specificity, self.parameter_count))
    }
}
