#![forbid(unsafe_code)]

#[path = "subscription_streaming/cases.rs"]
mod cases;
#[path = "subscription_streaming/client.rs"]
mod client;
#[path = "subscription_streaming/fixture.rs"]
mod fixture;
#[path = "subscription_streaming/server.rs"]
mod server;
#[path = "subscription_streaming/validation.rs"]
mod validation;
#[path = "subscription_streaming/websocket/mod.rs"]
mod websocket;

#[test]
fn official_services_drive_all_profiles_over_owned_loopback_connections() {
    cases::run();
}
