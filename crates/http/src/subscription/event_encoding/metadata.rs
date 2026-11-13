use std::ops::Range;

use chigiri_core::{Object, Value};

use crate::JsonConversionError;
use crate::response::write_value;
use crate::response_writer::BoundedWriter;

use super::{DELIVERY_COUNTER_WIDTH, SubscriptionEventEncodingError};

const PLACEHOLDER: &[u8; DELIVERY_COUNTER_WIDTH] = b"null            ";

pub(super) fn write_extensions(
    output: &mut BoundedWriter,
    extensions: &Object,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    output.byte(b'{')?;
    let mut chigiri_found = false;
    let mut slot = None;
    for (index, (name, value)) in extensions.iter().enumerate() {
        separator(output, index)?;
        member_name(output, name)?;
        if name == "chigiri" {
            chigiri_found = true;
            let Value::Object(chigiri) = value else {
                return Err(SubscriptionEventEncodingError::DeliveryExtensionCollision);
            };
            slot = Some(write_chigiri(output, chigiri)?);
        } else {
            write_value(output, value)?;
        }
    }
    if !chigiri_found {
        separator(output, extensions.len())?;
        member_name(output, "chigiri")?;
        slot = Some(write_missing_chigiri(output)?);
    }
    output.byte(b'}')?;
    Ok(slot.expect("chigiri extension is written exactly once"))
}

fn write_chigiri(
    output: &mut BoundedWriter,
    chigiri: &Object,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    output.byte(b'{')?;
    let mut delivery_found = false;
    let mut slot = None;
    for (index, (name, value)) in chigiri.iter().enumerate() {
        separator(output, index)?;
        member_name(output, name)?;
        if name == "delivery" {
            delivery_found = true;
            let Value::Object(delivery) = value else {
                return Err(SubscriptionEventEncodingError::DeliveryExtensionCollision);
            };
            slot = Some(write_delivery(output, delivery)?);
        } else {
            write_value(output, value)?;
        }
    }
    if !delivery_found {
        separator(output, chigiri.len())?;
        member_name(output, "delivery")?;
        slot = Some(write_missing_delivery(output)?);
    }
    output.byte(b'}')?;
    Ok(slot.expect("delivery extension is written exactly once"))
}

fn write_delivery(
    output: &mut BoundedWriter,
    delivery: &Object,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    output.byte(b'{')?;
    let mut counter_found = false;
    let mut slot = None;
    for (index, (name, value)) in delivery.iter().enumerate() {
        separator(output, index)?;
        member_name(output, name)?;
        if name == "droppedTotal" {
            counter_found = true;
            slot = Some(write_counter_slot(output)?);
        } else {
            write_value(output, value)?;
        }
    }
    if !counter_found {
        separator(output, delivery.len())?;
        member_name(output, "droppedTotal")?;
        slot = Some(write_counter_slot(output)?);
    }
    output.byte(b'}')?;
    Ok(slot.expect("delivery counter is written exactly once"))
}

fn write_missing_chigiri(
    output: &mut BoundedWriter,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    output.bytes(b"{\"delivery\":")?;
    let slot = write_missing_delivery(output)?;
    output.byte(b'}')?;
    Ok(slot)
}

fn write_missing_delivery(
    output: &mut BoundedWriter,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    output.bytes(b"{\"droppedTotal\":")?;
    let slot = write_counter_slot(output)?;
    output.byte(b'}')?;
    Ok(slot)
}

fn write_counter_slot(
    output: &mut BoundedWriter,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    let start = output.position();
    output.bytes(PLACEHOLDER)?;
    Ok(start..start + DELIVERY_COUNTER_WIDTH)
}

fn member_name(output: &mut BoundedWriter, name: &str) -> Result<(), JsonConversionError> {
    output.string(name)?;
    output.byte(b':')
}

fn separator(output: &mut BoundedWriter, index: usize) -> Result<(), JsonConversionError> {
    if index > 0 {
        output.byte(b',')?;
    }
    Ok(())
}
