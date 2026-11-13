//! Native future and private observer identify the concrete conversion task.

use std::sync::{Mutex, atomic::AtomicBool};

use super::*;
use crate::subscription::driver::tests::websocket_session::socket::Handle;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Transport,
    Protocol,
    Invariant,
    Panic,
    Observer,
    Payload,
    PendingDrop,
    ReadyDrop,
}
impl Mode {
    pub(super) fn poisoned(self) -> bool {
        matches!(self, Self::Payload | Self::PendingDrop | Self::ReadyDrop)
    }
}

pub(super) struct Probe {
    pub(super) app: Arc<State>,
    pub(super) peer: Handle,
    pub(super) calls: AtomicUsize,
    pub(super) drops: AtomicUsize,
    pub(super) pending: AtomicUsize,
    pub(super) errors: AtomicUsize,
    pub(super) disposed_in_owner: AtomicBool,
    pub(super) observed_in_owner: AtomicBool,
    owner: Mutex<Option<tokio::task::Id>>,
}
impl Probe {
    fn is_owner(&self) -> bool {
        *self.owner.lock().unwrap() == tokio::task::try_id()
    }
}

pub(super) fn fixture(mode: Mode, app: Arc<State>) -> (Endpoint<AppContext>, Arc<Probe>) {
    let probe = Arc::new(Probe {
        app: app.clone(),
        peer: Handle::default(),
        calls: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
        pending: AtomicUsize::new(0),
        errors: AtomicUsize::new(0),
        disposed_in_owner: AtomicBool::new(false),
        observed_in_owner: AtomicBool::new(false),
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
                .error_observer(Observe(probe.clone(), mode));
        })
        .build()
        .unwrap();
    (endpoint, probe)
}

struct Observe(Arc<Probe>, Mode);
impl ErrorObserver for Observe {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        assert_eq!(event.component(), "websocket-connection-init");
        self.0.errors.fetch_add(1, Ordering::SeqCst);
        self.0
            .observed_in_owner
            .store(self.0.is_owner(), Ordering::SeqCst);
        if matches!(self.1, Mode::Observer) {
            panic!("private native upgrade observer panic");
        }
    }
}

pub(super) struct Upgrade {
    probe: Arc<Probe>,
    mode: Mode,
    entered: bool,
}
impl Upgrade {
    pub(super) fn new(probe: Arc<Probe>, mode: Mode) -> Self {
        Self {
            probe,
            mode,
            entered: false,
        }
    }
}
impl Future for Upgrade {
    type Output = Result<Socket, WebSocketIoError>;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.entered {
            *self.probe.owner.lock().unwrap() = Some(tokio::task::id());
            self.probe.calls.fetch_add(1, Ordering::SeqCst);
            self.probe.app.wait_in_scalar();
            self.entered = true;
        }
        match self.mode {
            Mode::Transport => Poll::Ready(Err(WebSocketIoError::Transport)),
            Mode::Protocol => Poll::Ready(Err(WebSocketIoError::Protocol)),
            Mode::Invariant => Poll::Ready(Err(WebSocketIoError::Invariant)),
            Mode::Panic | Mode::Observer => panic!("private native upgrade poll panic"),
            Mode::Payload => std::panic::panic_any(Payload),
            Mode::PendingDrop => {
                self.probe.pending.store(1, Ordering::SeqCst);
                Poll::Pending
            }
            Mode::ReadyDrop => Poll::Ready(Ok(self.probe.peer.socket())),
        }
    }
}
impl Drop for Upgrade {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::SeqCst);
        self.probe
            .disposed_in_owner
            .store(self.probe.is_owner(), Ordering::SeqCst);
        if matches!(self.mode, Mode::PendingDrop | Mode::ReadyDrop) {
            panic!("private native upgrade future destructor panic");
        }
    }
}
struct Payload;
impl Drop for Payload {
    fn drop(&mut self) {
        panic!("private native upgrade panic payload destructor panic");
    }
}
