//! Native conversion retains one exact connection generation through disposal.

use super::{AdmittedWebSocketRequest, ClaimedWebSocketUpgrade, EndpointByteOwner};
use crate::__streaming::{WebSocketIo, WebSocketIoError};
use crate::BoxFuture;
use crate::managed::{EventActivity, HttpFaultReporter};
use crate::subscription::bridge_resources::{BridgeResourceError, DetachedCleanup};
use crate::subscription::worker::{
    ErasedWorkerCompletion, RetiredWorkerOwner, WorkerRetainedState, websocket::NativeUpgradeWorker,
};

pub(crate) struct RetainedWebSocketUpgrade {
    _request: AdmittedWebSocketRequest,
    _owner: EndpointByteOwner,
    _deadline: BoxFuture<'static, ()>,
}

impl ClaimedWebSocketUpgrade<'static> {
    pub(crate) fn bind_native_scope(
        &mut self,
        scope: &std::sync::Arc<crate::managed::SocketScope>,
    ) {
        self.connection.bind_socket_scope(scope);
    }
    pub(crate) fn native_physical_lease(
        &self,
    ) -> Result<
        crate::subscription::bridge_resources::WebSocketConnectionPhysicalLease,
        BridgeResourceError,
    > {
        self.connection.physical_lease()
    }

    pub(crate) fn native_worker<S: WebSocketIo>(
        &self,
        socket: BoxFuture<'static, Result<S, WebSocketIoError>>,
        activity: EventActivity,
        reporter: HttpFaultReporter,
    ) -> Result<NativeUpgradeWorker<S>, BridgeResourceError> {
        let instrumentation = self.instrumentation().map(|captured| {
            captured.handoff_scope(
                chigiri_runtime::instrumentation::InstrumentationKind::Provider {
                    phase: chigiri_runtime::LifecyclePhase::TransportAdmission,
                    component: "websocket-native-upgrade",
                },
            )
        });
        NativeUpgradeWorker::new(
            &self.connection,
            socket,
            activity,
            reporter,
            instrumentation,
        )
    }

    pub(crate) fn retire_native_upgrade(self, completion: ErasedWorkerCompletion) {
        let Self {
            request,
            connection,
            owner,
            deadline,
        } = self;
        let retained = WorkerRetainedState::NativeUpgrade {
            _retained: Box::new(RetainedWebSocketUpgrade {
                _request: request,
                _owner: owner,
                _deadline: deadline,
            }),
        };
        match connection.revoke() {
            Ok(terminal) => terminal.quarantine(DetachedCleanup::worker(completion, retained)),
            Err(_) => completion.retire(RetiredWorkerOwner::orphan(retained)),
        }
    }
}
