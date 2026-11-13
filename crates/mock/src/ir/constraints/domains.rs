//! Validate compatible numeric, string, and list constraint domains before startup.

use chigiri::core::Span;

use super::{Constraints, Representation, parse::invalid};
use crate::ir::{TypeNode, TypeRef};
use crate::schema::SchemaError;
use crate::value::{decimal, work::Work};

pub(crate) fn key(c: &Constraints, span: Span) -> Result<(), SchemaError> {
    if c.max_length == Some(0) {
        return Err(invalid(span));
    }
    // The identity and the alias reference keys have the same non-empty 256-byte domain.
    strings(c, None, true, span)
}

pub(super) fn validate(
    c: &Constraints,
    named: &TypeNode,
    ty: &TypeRef,
    shape: Option<Representation>,
    span: Span,
    work: &mut Work,
) -> Result<(), SchemaError> {
    let string = c.min_length.is_some()
        || c.max_length.is_some()
        || c.starts_with.is_some()
        || c.ends_with.is_some()
        || c.contains.is_some()
        || c.not_contains.is_some()
        || c.pattern.is_some()
        || c.format.is_some();
    let numeric = c.min.is_some()
        || c.max.is_some()
        || c.exclusive_min.is_some()
        || c.exclusive_max.is_some()
        || c.multiple_of.is_some();
    let items = c.min_items.is_some() || c.max_items.is_some();
    if (string && shape != Some(Representation::String))
        || (numeric
            && !matches!(
                shape,
                Some(Representation::Integer | Representation::Number)
            ))
        || (items && ty.list_depth() == 0 && shape != Some(Representation::Array))
        || c.multiple_of.is_some_and(|value| value <= 0.0)
    {
        return Err(invalid(span));
    }
    let scalar = named.scalar.as_ref().map(|s| &s.constraints);
    numbers(
        c,
        scalar,
        shape == Some(Representation::Integer),
        named.name.as_ref() == "Int",
        span,
        work,
    )?;
    strings(c, scalar, named.name.as_ref() == "ID", span)?;
    let base = if ty.list_depth() == 0 { scalar } else { None };
    lengths(
        c.min_items,
        c.max_items,
        base.and_then(|s| s.min_items),
        base.and_then(|s| s.max_items),
        crate::value::MAX_ITEMS,
        span,
    )
}

pub(super) fn basic(c: &Constraints, shape: Representation, span: Span) -> Result<(), SchemaError> {
    if ((c.min.is_some() || c.max.is_some())
        && !matches!(shape, Representation::Integer | Representation::Number))
        || ((c.min_bytes.is_some() || c.max_bytes.is_some()) && shape != Representation::String)
        || ((c.min_items.is_some() || c.max_items.is_some()) && shape != Representation::Array)
        || c.max_bytes
            .is_some_and(|n| n > crate::value::MAX_STRING_BYTES)
        || c.max_items.is_some_and(|n| n > crate::value::MAX_ITEMS)
    {
        return Err(invalid(span));
    }
    numbers(
        c,
        None,
        shape == Representation::Integer,
        false,
        span,
        &mut Work::default(),
    )?;
    lengths(
        c.min_bytes,
        c.max_bytes,
        None,
        None,
        crate::value::MAX_STRING_BYTES,
        span,
    )?;
    lengths(
        c.min_items,
        c.max_items,
        None,
        None,
        crate::value::MAX_ITEMS,
        span,
    )
}

fn numbers(
    c: &Constraints,
    base: Option<&Constraints>,
    integer: bool,
    builtin: bool,
    span: Span,
    work: &mut Work,
) -> Result<(), SchemaError> {
    let mut low = if builtin {
        f64::from(i32::MIN)
    } else {
        -f64::MAX
    };
    let mut high = if builtin {
        f64::from(i32::MAX)
    } else {
        f64::MAX
    };
    for bounds in std::iter::once(c).chain(base) {
        if let Some(min) = bounds.min {
            low = low.max(min);
        }
        if let Some(max) = bounds.max {
            high = high.min(max);
        }
        if let Some(min) = bounds.exclusive_min {
            low = low.max(min.next_up());
        }
        if let Some(max) = bounds.exclusive_max {
            high = high.min(max.next_down());
        }
    }
    if integer {
        low = low.ceil();
        high = high.floor();
    }
    if low > high || !low.is_finite() || !high.is_finite() {
        return Err(invalid(span));
    }
    if let Some(divisor) = c.multiple_of {
        let intersects =
            decimal::intersects(low, high, divisor, integer, work).map_err(|_| invalid(span))?;
        if !intersects {
            return Err(invalid(span));
        }
    }
    Ok(())
}

fn strings(
    c: &Constraints,
    base: Option<&Constraints>,
    id: bool,
    span: Span,
) -> Result<(), SchemaError> {
    let bytes = base
        .and_then(|s| s.max_bytes)
        .unwrap_or(crate::value::MAX_STRING_BYTES)
        .min(if id {
            crate::limits::ID_BYTES
        } else {
            crate::value::MAX_STRING_BYTES
        });
    lengths(c.min_length, c.max_length, None, None, bytes, span)?;
    if c.max_length.is_some_and(|max| {
        base.and_then(|s| s.min_bytes)
            .is_some_and(|min| min > max.saturating_mul(4))
    }) || c.not_contains.as_deref() == Some("")
    {
        return Err(invalid(span));
    }
    for required in [&c.starts_with, &c.ends_with, &c.contains]
        .into_iter()
        .flatten()
    {
        if required.len() > bytes
            || c.max_length
                .is_some_and(|max| required.chars().count() > max)
            || c.not_contains
                .as_ref()
                .is_some_and(|no| required.contains(no.as_ref()))
        {
            return Err(invalid(span));
        }
    }
    Ok(())
}

fn lengths(
    min: Option<usize>,
    max: Option<usize>,
    base_min: Option<usize>,
    base_max: Option<usize>,
    cap: usize,
    span: Span,
) -> Result<(), SchemaError> {
    let low = min.unwrap_or(0).max(base_min.unwrap_or(0));
    let high = max.unwrap_or(cap).min(base_max.unwrap_or(cap)).min(cap);
    if low > high {
        Err(invalid(span))
    } else {
        Ok(())
    }
}
