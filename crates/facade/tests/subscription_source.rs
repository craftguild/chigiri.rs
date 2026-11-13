use chigiri::{SubscriptionOverflow, SubscriptionSource};

fn consume_facade_source(_: Option<SubscriptionSource<u8, ()>>) {}

#[test]
fn facade_reexports_the_subscription_source_contract() {
    consume_facade_source(None);
    assert!(matches!(
        SubscriptionOverflow::DropOldest,
        SubscriptionOverflow::DropOldest
    ));
}
