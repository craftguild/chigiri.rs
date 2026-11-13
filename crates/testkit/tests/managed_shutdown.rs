#![forbid(unsafe_code)]

mod managed_shutdown {
    pub(super) mod cases;
    mod client;
    pub(super) mod failures;
    mod fixture;
    mod hyper_server;
}

#[test]
fn public_hooks_order_sdk_shutdown_before_native_server_stop() {
    managed_shutdown::cases::run();
}

#[test]
fn public_native_subscription_failures_preserve_the_committed_http_boundary() {
    managed_shutdown::failures::run();
}
