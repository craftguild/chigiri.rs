use chigiri_core::{GraphQlError, PathSegment};

use super::{formatted, member, separator, string, write_object};
use crate::JsonConversionError;
use crate::response_writer::BoundedWriter;

#[derive(Clone, Copy)]
pub(crate) enum ErrorProjection {
    GraphQl,
    TopLevelTransport,
}

pub(crate) fn write_errors(
    output: &mut BoundedWriter,
    errors: &[GraphQlError],
    projection: ErrorProjection,
) -> Result<(), JsonConversionError> {
    output.byte(b'[')?;
    for (index, error) in errors.iter().enumerate() {
        separator(output, index)?;
        write_error(output, error, projection)?;
    }
    output.byte(b']')
}

fn write_error(
    output: &mut BoundedWriter,
    error: &GraphQlError,
    projection: ErrorProjection,
) -> Result<(), JsonConversionError> {
    output.byte(b'{')?;
    let mut comma = false;
    member(output, &mut comma, "message")?;
    string(output, &error.message)?;
    if matches!(projection, ErrorProjection::GraphQl) && !error.locations.is_empty() {
        member(output, &mut comma, "locations")?;
        output.byte(b'[')?;
        for (index, location) in error.locations.iter().enumerate() {
            separator(output, index)?;
            formatted(
                output,
                format_args!(
                    "{{\"line\":{},\"column\":{}}}",
                    location.line, location.column
                ),
            )?;
        }
        output.byte(b']')?;
    }
    if matches!(projection, ErrorProjection::GraphQl) && !error.path.is_empty() {
        member(output, &mut comma, "path")?;
        output.byte(b'[')?;
        for (index, segment) in error.path.iter().enumerate() {
            separator(output, index)?;
            match segment {
                PathSegment::Field(field) => string(output, field)?,
                PathSegment::Index(index) => formatted(output, format_args!("{index}"))?,
            }
        }
        output.byte(b']')?;
    }
    if !error.extensions.is_empty() {
        member(output, &mut comma, "extensions")?;
        write_object(output, &error.extensions)?;
    }
    output.byte(b'}')?;
    Ok(())
}
