//! Release staging capacity only after the final retained batch owner leaves.

use super::DispatchReservation;

impl Drop for DispatchReservation {
    fn drop(&mut self) {
        let Ok(mut usage) = self.usage.values.lock() else {
            return;
        };
        let Some(slots) = usage.slots.checked_sub(1) else {
            return;
        };
        let Some(bytes) = usage.bytes.checked_sub(self.bytes) else {
            return;
        };
        usage.slots = slots;
        usage.bytes = bytes;
    }
}
