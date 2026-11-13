//! Focused operation-slot ownership and transition tests.

mod delivery;
mod fixture;
mod terminal;

use super::failure::is_shared_delivery;
use crate::subscription::outbox::SubscriptionOutboxSignal;

#[test]
fn only_a_physical_send_stall_is_a_shared_delivery_failure() {
    assert!(is_shared_delivery(SubscriptionOutboxSignal::SendStalled));
    assert!(!is_shared_delivery(
        SubscriptionOutboxSignal::QueueAgeExpired
    ));
    assert!(!is_shared_delivery(
        SubscriptionOutboxSignal::OversizedEvent
    ));
    assert!(!is_shared_delivery(
        SubscriptionOutboxSignal::DeliveryExtensionCollision
    ));
    assert!(!is_shared_delivery(
        SubscriptionOutboxSignal::SharedBudgetPressureExpired
    ));
}
