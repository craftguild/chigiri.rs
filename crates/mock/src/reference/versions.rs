//! Accept only the current closed version set when reading a full reference document.

use serde::Deserialize;

pub(super) fn current_format<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<String, D::Error> {
    exact(
        reader,
        crate::info::REFERENCE_FORMAT,
        "unsupported reference format",
    )
}

pub(super) fn current_api<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    exact(reader, crate::info::API_VERSION, "unsupported host API")
}

pub(super) fn current_project<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<String, D::Error> {
    exact(reader, crate::project::FORMAT, "unsupported project format")
}

pub(super) fn current_bundle<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<String, D::Error> {
    exact(
        reader,
        crate::info::CONFIG_FORMAT,
        "unsupported bundle format",
    )
}

pub(super) fn current_mock<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(reader)?;
    if value.as_ref().is_some_and(|format| {
        format != crate::project::FORMAT && format != crate::info::CONFIG_FORMAT
    }) {
        return Err(serde::de::Error::custom("unsupported mock format"));
    }
    Ok(value)
}

fn exact<'de, D: serde::Deserializer<'de>>(
    reader: D,
    expected: &str,
    message: &str,
) -> Result<String, D::Error> {
    let value = String::deserialize(reader)?;
    if value != expected {
        return Err(serde::de::Error::custom(message));
    }
    Ok(value)
}
