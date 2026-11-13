use chigiri_core::{GraphQlError, GraphQlResponse, Location};
use chigiri_http::{
    __encode_subscription_control, EncodedSubscriptionControl, SubscriptionControl,
    SubscriptionTransport,
};

pub(super) const LARGE_LIMIT: usize = 128 * 1024;

pub(super) fn encode(
    transport: SubscriptionTransport,
    control: SubscriptionControl<'_>,
    maximum_bytes: usize,
) -> EncodedSubscriptionControl {
    __encode_subscription_control(transport, control, maximum_bytes).unwrap()
}

pub(super) fn terminal_response() -> GraphQlResponse {
    let mut error = GraphQlError::new("terminal failure").code("TERMINAL");
    error.locations.push(Location::new(3, 8));
    error.path.push("viewer".into());
    error.path.push(1_usize.into());
    error.extensions.insert("safe", true);
    GraphQlResponse::request_error(error)
}

pub(super) fn assert_owner_contract() {
    assert_owner::<EncodedSubscriptionControl>();
    let _ = <EncodedSubscriptionControl as AmbiguousIfClone<_>>::marker;
}

fn assert_owner<T: AsRef<[u8]> + Send + 'static>() {}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}

struct Invalid;

impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
