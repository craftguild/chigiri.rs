use std::sync::Arc;

use chigiri_core::{Object, Value};
use serde_json::value::RawValue;

use super::error::WebSocketClientMessageError;
use super::raw;
use super::types::{WebSocketControlPayload, WebSocketObjectPayload};
use crate::{RequestEnvelope, ResponseMediaType};

const MAX_OBJECT_PAYLOAD_BYTES: usize = 16 * 1024;

pub(super) fn authorization_object(
    raw: Option<&RawValue>,
) -> Result<WebSocketObjectPayload, WebSocketClientMessageError> {
    let Some(raw) = raw else {
        return Ok(WebSocketObjectPayload::new(None));
    };
    let text = raw.get().trim_ascii();
    if text == "null" {
        return Ok(WebSocketObjectPayload::new(None));
    }
    if !text.starts_with('{') {
        return Err(WebSocketClientMessageError::malformed());
    }
    if raw.get().len() > MAX_OBJECT_PAYLOAD_BYTES {
        return Err(WebSocketClientMessageError::payload_too_large());
    }
    let Value::Object(object) = decode_value(raw)? else {
        return Err(WebSocketClientMessageError::malformed());
    };
    Ok(WebSocketObjectPayload::new(Some(Arc::new(object))))
}

pub(super) fn control_object(
    raw: Option<&RawValue>,
) -> Result<WebSocketControlPayload, WebSocketClientMessageError> {
    let Some(raw) = raw else {
        return Ok(WebSocketControlPayload::new(None));
    };
    let text = raw.get().trim_ascii();
    if text == "null" {
        return Ok(WebSocketControlPayload::new(None));
    }
    if !text.starts_with('{') {
        return Err(WebSocketClientMessageError::malformed());
    }
    if raw.get().len() > MAX_OBJECT_PAYLOAD_BYTES {
        return Err(WebSocketClientMessageError::payload_too_large());
    }
    Ok(WebSocketControlPayload::new(Some(
        raw.get().to_owned().into_boxed_str(),
    )))
}

pub(super) fn subscribe(raw: &RawValue) -> Result<RequestEnvelope, WebSocketClientMessageError> {
    let payload = raw::subscribe(raw).map_err(|()| WebSocketClientMessageError::malformed())?;
    let mut object = Object::new();
    let query = payload
        .query
        .ok_or_else(WebSocketClientMessageError::malformed)
        .and_then(decode_value)?;
    if !matches!(query, Value::String(_)) {
        return Err(WebSocketClientMessageError::malformed());
    }
    object.insert("query", query);
    insert_nullable_string(&mut object, "operationName", payload.operation_name)?;
    insert_nullable_object(&mut object, "variables", payload.variables)?;
    insert_nullable_object(&mut object, "extensions", payload.extensions)?;
    crate::request::envelope_from_object(object, ResponseMediaType::GraphQlResponseJson)
        .map_err(|_| WebSocketClientMessageError::malformed())
}

fn insert_nullable_string(
    object: &mut Object,
    name: &'static str,
    raw: Option<&RawValue>,
) -> Result<(), WebSocketClientMessageError> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let value = decode_value(raw)?;
    if !matches!(value, Value::Null | Value::String(_)) {
        return Err(WebSocketClientMessageError::malformed());
    }
    object.insert(name, value);
    Ok(())
}

fn insert_nullable_object(
    object: &mut Object,
    name: &'static str,
    raw: Option<&RawValue>,
) -> Result<(), WebSocketClientMessageError> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let value = decode_value(raw)?;
    if !matches!(value, Value::Null | Value::Object(_)) {
        return Err(WebSocketClientMessageError::malformed());
    }
    object.insert(name, value);
    Ok(())
}

fn decode_value(raw: &RawValue) -> Result<Value, WebSocketClientMessageError> {
    let value =
        serde_json::from_str(raw.get()).map_err(|_| WebSocketClientMessageError::malformed())?;
    crate::value_from_json(value).map_err(|_| WebSocketClientMessageError::malformed())
}
