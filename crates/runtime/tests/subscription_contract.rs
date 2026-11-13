//! Focused contracts for the hidden sequential subscription driver.

#[path = "subscription_contract/cancellation.rs"]
mod cancellation;
#[path = "subscription_contract/driver.rs"]
mod driver;
#[path = "subscription_contract/errors.rs"]
mod errors;
#[path = "subscription_contract/fixture.rs"]
mod fixture;
#[path = "subscription_contract/framework.rs"]
mod framework;
#[path = "subscription_contract/lifecycle.rs"]
mod lifecycle;
#[path = "subscription_contract/middleware.rs"]
mod middleware;
#[path = "subscription_contract/websocket.rs"]
mod websocket;
