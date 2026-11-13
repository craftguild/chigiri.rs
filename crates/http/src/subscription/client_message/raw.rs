use std::fmt;

use serde::de::{Error, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

pub(super) struct RawMessage<'a> {
    pub(super) kind: String,
    pub(super) id: Option<&'a RawValue>,
    pub(super) payload: Option<&'a RawValue>,
}

pub(super) struct RawSubscribe<'a> {
    pub(super) query: Option<&'a RawValue>,
    pub(super) operation_name: Option<&'a RawValue>,
    pub(super) variables: Option<&'a RawValue>,
    pub(super) extensions: Option<&'a RawValue>,
}

pub(super) fn message(text: &str) -> Result<RawMessage<'_>, ()> {
    let mut decoder = serde_json::Deserializer::from_str(text);
    let message = decoder.deserialize_map(MessageVisitor).map_err(|_| ())?;
    decoder.end().map_err(|_| ())?;
    Ok(message)
}

pub(super) fn subscribe(raw: &RawValue) -> Result<RawSubscribe<'_>, ()> {
    let mut decoder = serde_json::Deserializer::from_str(raw.get());
    let payload = decoder.deserialize_map(SubscribeVisitor).map_err(|_| ())?;
    decoder.end().map_err(|_| ())?;
    Ok(payload)
}

#[derive(Clone, Copy)]
enum MessageField {
    Type,
    Id,
    Payload,
    Other,
}

impl<'de> Deserialize<'de> for MessageField {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        decoder.deserialize_identifier(MessageFieldVisitor)
    }
}

struct MessageFieldVisitor;

impl Visitor<'_> for MessageFieldVisitor {
    type Value = MessageField;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a WebSocket message member")
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(match value {
            "type" => MessageField::Type,
            "id" => MessageField::Id,
            "payload" => MessageField::Payload,
            _ => MessageField::Other,
        })
    }
}

struct MessageVisitor;

impl<'de> Visitor<'de> for MessageVisitor {
    type Value = RawMessage<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GraphQL WebSocket message object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut kind = None;
        let mut id = None;
        let mut payload = None;
        let mut id_seen = false;
        let mut payload_seen = false;
        while let Some(field) = map.next_key::<MessageField>()? {
            match field {
                MessageField::Type => {
                    if kind.is_some() {
                        return Err(A::Error::custom("duplicate message type"));
                    }
                    kind = Some(map.next_value::<String>()?);
                }
                MessageField::Id => {
                    if id_seen {
                        return Err(A::Error::custom("duplicate message id"));
                    }
                    id_seen = true;
                    id = Some(map.next_value::<&RawValue>()?);
                }
                MessageField::Payload => {
                    if payload_seen {
                        return Err(A::Error::custom("duplicate message payload"));
                    }
                    payload_seen = true;
                    payload = Some(map.next_value::<&RawValue>()?);
                }
                MessageField::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        let kind = kind.ok_or_else(|| A::Error::custom("missing message type"))?;
        Ok(RawMessage { kind, id, payload })
    }
}

#[derive(Clone, Copy)]
enum SubscribeField {
    Query,
    OperationName,
    Variables,
    Extensions,
    Other,
}

impl<'de> Deserialize<'de> for SubscribeField {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        decoder.deserialize_identifier(SubscribeFieldVisitor)
    }
}

struct SubscribeFieldVisitor;

impl Visitor<'_> for SubscribeFieldVisitor {
    type Value = SubscribeField;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a subscribe payload member")
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(match value {
            "query" => SubscribeField::Query,
            "operationName" => SubscribeField::OperationName,
            "variables" => SubscribeField::Variables,
            "extensions" => SubscribeField::Extensions,
            _ => SubscribeField::Other,
        })
    }
}

struct SubscribeVisitor;

impl<'de> Visitor<'de> for SubscribeVisitor {
    type Value = RawSubscribe<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GraphQL subscribe payload object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut query = None;
        let mut operation_name = None;
        let mut variables = None;
        let mut extensions = None;
        while let Some(field) = map.next_key::<SubscribeField>()? {
            let slot = match field {
                SubscribeField::Query => &mut query,
                SubscribeField::OperationName => &mut operation_name,
                SubscribeField::Variables => &mut variables,
                SubscribeField::Extensions => &mut extensions,
                SubscribeField::Other => {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
            };
            if slot.is_some() {
                return Err(A::Error::custom("duplicate subscribe payload member"));
            }
            *slot = Some(map.next_value::<&RawValue>()?);
        }
        Ok(RawSubscribe {
            query,
            operation_name,
            variables,
            extensions,
        })
    }
}
