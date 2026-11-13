use std::future::pending;
use std::sync::Arc;
use std::task::{Context, Waker};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};
use chigiri_runtime::{GraphQlResponse, RequestTelemetry};

use super::super::{WebSocketOperationDeliverySeed, WebSocketOperationSlot};
use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeConnection, BridgeOperationKind, BridgeResourceOwner,
};
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::subscription::worker::websocket::{WebSocketTerminalCarrier, WebSocketTerminalMessage};

struct NeverDeadlines;

impl DeadlineFactory for NeverDeadlines {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(pending())
    }
}

static DEADLINES: NeverDeadlines = NeverDeadlines;

pub(super) struct Fixture {
    pub(super) connection: BridgeConnection,
    pub(super) controls: WebSocketControlStaging,
    pub(super) settings: ValidatedSubscriptionSettings,
    pub(super) bytes: crate::subscription::delivery::EndpointByteOwner,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let settings = ValidatedSubscriptionSettings::new(
            SubscriptionLimits {
                max_endpoint_encoded_bytes: 64 * 1024,
                ..SubscriptionLimits::default()
            },
            SubscriptionProtocolSettings::default(),
        );
        let owner = BridgeResourceOwner::new(&settings).unwrap();
        let bytes = owner.delivery().byte_owner();
        let controls = WebSocketControlStaging::new(bytes.clone());
        let connection = owner.open_connection().unwrap();
        Self {
            connection,
            controls,
            settings,
            bytes,
        }
    }

    pub(super) fn active(&self, id: Arc<str>) -> ActiveBridgeOperation {
        self.connection
            .begin(id)
            .unwrap()
            .activate(BridgeOperationKind::Query)
            .unwrap()
    }

    pub(super) fn slot(
        &self,
        id: Arc<str>,
        active: ActiveBridgeOperation,
        message: WebSocketTerminalMessage,
    ) -> WebSocketOperationSlot<()> {
        let carrier =
            WebSocketTerminalCarrier::joined_for_test(active, message, &DEADLINES).unwrap();
        WebSocketOperationSlot::from_terminal(
            id,
            carrier,
            WebSocketOperationDeliverySeed::new(self.settings, self.bytes.clone()),
            SubscriptionFailureReporter::new(
                crate::subscription::request::test_support::schema(),
                RequestTelemetry::default(),
                "websocket-operation-slot-test",
            ),
        )
    }

    pub(super) fn slot_from_carrier(
        &self,
        id: Arc<str>,
        carrier: WebSocketTerminalCarrier,
    ) -> WebSocketOperationSlot<()> {
        WebSocketOperationSlot::from_terminal(
            id,
            carrier,
            WebSocketOperationDeliverySeed::new(self.settings, self.bytes.clone()),
            SubscriptionFailureReporter::new(
                crate::subscription::request::test_support::schema(),
                RequestTelemetry::default(),
                "websocket-operation-slot-test",
            ),
        )
    }
}

pub(super) fn context() -> Context<'static> {
    Context::from_waker(Waker::noop())
}

pub(super) fn error() -> GraphQlResponse {
    GraphQlResponse::request_error(
        chigiri_runtime::GraphQlError::new("Operation failed").code("OPERATION_FAILED"),
    )
}
