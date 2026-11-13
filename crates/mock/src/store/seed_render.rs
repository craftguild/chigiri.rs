//! Render borrowed seed values directly into a bounded sink without building a second large DOM.

use chigiri::core::Object;

pub(super) fn render(values: &Object) -> Result<String, &'static str> {
    crate::value::serialize::render(
        &crate::value::serialize::BorrowedObject(values),
        crate::limits::SEED_BYTES,
    )
    .map_err(|_| "canonical seed byte limit or number")
}
