//! Application-selected cleanup also works before any upgrade/Mock dispatch.

use std::time::Duration;

use super::endpoint;
use crate::streaming::streaming_service;

#[tokio::test]
async fn application_transport_failure_or_shutdown_closes_an_unpolled_upgrade_driver() {
    for shutdown in [false, true] {
        let (service, http_driver, guard) = streaming_service(endpoint()).unwrap();
        let (upgrade, mut driver) = service.websocket_connection();
        if shutdown {
            driver.shutdown();
            driver.transport_closed();
        } else {
            driver.transport_closed();
            driver.shutdown();
        }
        tokio::time::timeout(Duration::from_secs(3), driver)
            .await
            .unwrap()
            .unwrap();
        drop((upgrade, service));
        tokio::time::timeout(Duration::from_secs(3), http_driver)
            .await
            .unwrap()
            .unwrap();
        guard.shutdown().unwrap().await.unwrap();
    }
}
