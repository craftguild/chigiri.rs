//! Faulting socket and observer identify their original SDK task.

use super::*;
use crate::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketWireFrame,
    WebSocketWriteProgress,
};
use crate::subscription::driver::tests::websocket_session::socket::Handle;
use std::sync::{Mutex, atomic::AtomicBool};

#[derive(Clone, Copy)]
pub(super) enum Method {
    Receive,
    Ready,
    Start,
    Write,
    Close,
    Drop,
}
#[derive(Clone, Copy)]
pub(super) enum Fault {
    Panic,
    Invariant,
    Payload,
    Observer,
    AckDrop,
    Transport,
    ConstructorDrop,
}
pub(super) struct Probe {
    pub(super) peer: Handle,
    pub(super) app: Arc<State>,
    pub(super) armed: AtomicBool,
    pub(super) errors: AtomicUsize,
    pub(super) drops: AtomicUsize,
    pub(super) observed_in_owner: AtomicBool,
    pub(super) dropped_in_owner: AtomicBool,
    owner: Mutex<Option<tokio::task::Id>>,
}
impl Probe {
    fn is_owner(&self) -> bool {
        *self.owner.lock().unwrap() == tokio::task::try_id()
    }
}
pub(super) fn fixture(fault: Fault) -> (Endpoint<AppContext>, Arc<Probe>) {
    let app = State::new(Vec::new());
    let probe = Arc::new(Probe {
        peer: Handle::default(),
        app: app.clone(),
        armed: AtomicBool::new(false),
        errors: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
        observed_in_owner: AtomicBool::new(false),
        dropped_in_owner: AtomicBool::new(false),
        owner: Mutex::new(None),
    });
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(app.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(Factory(app))
        .inject(|components| {
            components
                .observability()
                .error_observer(Observe(probe.clone(), fault));
        })
        .build()
        .unwrap();
    (endpoint, probe)
}
struct Observe(Arc<Probe>, Fault);
impl ErrorObserver for Observe {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        assert_eq!(event.component(), "managed-websocket-session");
        self.0.errors.fetch_add(1, Ordering::SeqCst);
        self.0
            .observed_in_owner
            .store(self.0.is_owner(), Ordering::SeqCst);
        if matches!(self.1, Fault::Observer) {
            panic!("private native socket observer failure");
        }
    }
}
pub(super) struct Native {
    socket: Socket,
    probe: Arc<Probe>,
    method: Method,
    fault: Fault,
}
pub(super) fn native(probe: Arc<Probe>, method: Method, fault: Fault) -> Native {
    Native {
        socket: probe.peer.socket(),
        probe,
        method,
        fault,
    }
}
impl Native {
    fn fault(&self, method: Method) -> Result<(), WebSocketIoError> {
        *self.probe.owner.lock().unwrap() = Some(tokio::task::id());
        if !self.probe.armed.load(Ordering::SeqCst)
            || std::mem::discriminant(&self.method) != std::mem::discriminant(&method)
        {
            return Ok(());
        }
        match self.fault {
            Fault::Invariant => Err(WebSocketIoError::Invariant),
            Fault::Transport => Err(WebSocketIoError::Transport),
            Fault::Payload => std::panic::panic_any(Payload),
            Fault::Panic | Fault::Observer | Fault::AckDrop | Fault::ConstructorDrop => {
                panic!("private native socket poll failure")
            }
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::SeqCst);
        self.probe
            .dropped_in_owner
            .store(self.probe.is_owner(), Ordering::SeqCst);
        let _ = self.fault(Method::Drop);
    }
}
impl WebSocketIo for Native {
    fn poll_receive(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        self.fault(Method::Receive)?;
        Pin::new(&mut self.socket).poll_receive(cx)
    }
    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        self.fault(Method::Ready)?;
        Pin::new(&mut self.socket).poll_ready(cx)
    }
    fn start_send(
        mut self: Pin<&mut Self>,
        frame: WebSocketWireFrame,
    ) -> Result<(), WebSocketIoError> {
        self.fault(Method::Start)?;
        Pin::new(&mut self.socket).start_send(frame)
    }
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        self.fault(Method::Write)?;
        Pin::new(&mut self.socket).poll_write(cx)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        self.fault(Method::Close)?;
        Pin::new(&mut self.socket).poll_close(cx, close)
    }
}
struct Payload;
pub(super) struct FailCleanupTimer;
pub(super) static FAIL_CLEANUP_TIMER: FailCleanupTimer = FailCleanupTimer;
impl crate::DeadlineFactory for FailCleanupTimer {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if duration == Duration::from_secs(5) {
            panic!("private cleanup timer construction failure");
        }
        Box::pin(tokio::time::sleep(duration))
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        panic!("private native socket panic payload destructor failure");
    }
}
