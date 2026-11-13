//! Authorization errors and future destructors identify their original task.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::*;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Reject,
    Error,
    Observer,
    ErrorDrop,
    Both,
    Payload,
    PendingFuture,
    ReadyFuture,
}

pub(super) struct Probe {
    pub(super) app: Arc<State>,
    pub(super) calls: AtomicUsize,
    pub(super) errors: AtomicUsize,
    pub(super) drops: AtomicUsize,
    pub(super) observed_in_owner: AtomicBool,
    pub(super) disposed_in_owner: AtomicBool,
    pub(super) pending: AtomicBool,
    pub(super) blocking_drop: Mutex<Option<Arc<State>>>,
    owner: Mutex<Option<tokio::task::Id>>,
}

impl Probe {
    fn enter(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.owner.lock().unwrap() = Some(tokio::task::id());
        self.app.wait_in_scalar();
    }

    fn is_owner(&self) -> bool {
        *self.owner.lock().unwrap() == tokio::task::try_id()
    }

    fn disposed(&self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        self.disposed_in_owner
            .store(self.is_owner(), Ordering::SeqCst);
    }
}

pub(super) fn fixture(mode: Mode, app: Arc<State>) -> (Endpoint<AppContext>, Arc<Probe>) {
    let probe = Arc::new(Probe {
        app: app.clone(),
        calls: AtomicUsize::new(0),
        errors: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
        observed_in_owner: AtomicBool::new(false),
        disposed_in_owner: AtomicBool::new(false),
        pending: AtomicBool::new(false),
        blocking_drop: Mutex::new(None),
        owner: Mutex::new(None),
    });
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(app.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(Factory(app))
        .connection_init_authorizer(Authorize(probe.clone(), mode))
        .inject(|components| {
            components
                .observability()
                .error_observer(Observe(probe.clone(), mode));
        })
        .build()
        .unwrap();
    (endpoint, probe)
}

struct Authorize(Arc<Probe>, Mode);
impl ConnectionInitAuthorizer for Authorize {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        if matches!(self.1, Mode::PendingFuture | Mode::ReadyFuture) {
            return Box::pin(BadFuture {
                probe: self.0.clone(),
                mode: self.1,
                entered: false,
            });
        }
        Box::pin(async move {
            self.0.enter();
            match self.1 {
                Mode::Reject => Ok(ConnectionInitDecision::Reject),
                Mode::Payload => std::panic::panic_any(Payload(self.0.clone())),
                _ => Err(ConnectionInitError::new(AppError(self.0.clone(), self.1))),
            }
        })
    }
}

struct Observe(Arc<Probe>, Mode);
impl ErrorObserver for Observe {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        assert_eq!(event.component(), "websocket-connection-init");
        self.0.errors.fetch_add(1, Ordering::SeqCst);
        self.0
            .observed_in_owner
            .store(self.0.is_owner(), Ordering::SeqCst);
        if matches!(self.1, Mode::Observer | Mode::Both) {
            panic!("private authorization observer panic");
        }
    }
}

struct AppError(Arc<Probe>, Mode);
impl std::fmt::Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppError")
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private authorization error")
    }
}
impl std::error::Error for AppError {}
impl Drop for AppError {
    fn drop(&mut self) {
        self.0.disposed();
        let blocking = self.0.blocking_drop.lock().unwrap().clone();
        if let Some(blocking) = blocking {
            blocking.wait_in_scalar();
        }
        if matches!(self.1, Mode::ErrorDrop | Mode::Both) {
            panic!("private authorization error destructor panic");
        }
    }
}

struct Payload(Arc<Probe>);
impl Drop for Payload {
    fn drop(&mut self) {
        self.0.disposed();
        panic!("private authorization panic payload destructor failure");
    }
}

struct BadFuture {
    probe: Arc<Probe>,
    mode: Mode,
    entered: bool,
}
impl Future for BadFuture {
    type Output = Result<ConnectionInitDecision, ConnectionInitError>;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.entered {
            self.probe.enter();
            self.entered = true;
        }
        if matches!(self.mode, Mode::ReadyFuture) {
            Poll::Ready(Ok(ConnectionInitDecision::Accept))
        } else {
            self.probe.pending.store(true, Ordering::SeqCst);
            Poll::Pending
        }
    }
}
impl Drop for BadFuture {
    fn drop(&mut self) {
        self.probe.disposed();
        panic!("private authorization future destructor panic");
    }
}
