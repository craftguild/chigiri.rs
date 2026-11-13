//! Explicit application failure modes and context-sensitive destruction probes.

use std::sync::Arc;

use chigiri_http::RequestPhase;

use super::fixture::Probe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Success,
    RejectFactory,
    InternalFactory,
    PendingFactory,
    PanicFactory,
    PendingResolver,
    PanicResolver,
    OwnedPanicResolver,
    ErrorFirstResolver,
}

struct Lease {
    probe: Arc<Probe>,
    phase: RequestPhase,
    stage: &'static str,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.probe.observe(self.stage, self.phase, None);
    }
}

#[derive(Debug)]
struct PrivateError;
impl std::fmt::Display for PrivateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private event failure")
    }
}
impl std::error::Error for PrivateError {}

pub(super) async fn prepare(
    probe: &Arc<Probe>,
    phase: RequestPhase,
) -> Result<(), chigiri::PrepareError> {
    if !matches!(phase, RequestPhase::SubscriptionEvent { .. }) {
        return Ok(());
    }
    match probe.mode {
        Mode::RejectFactory => Err(chigiri::PrepareError::Reject(
            chigiri_http::Rejection::new(403, chigiri::GraphQlError::new("event denied")).unwrap(),
        )),
        Mode::InternalFactory => Err(chigiri::PrepareError::Internal(chigiri::HookError::new(
            PrivateError,
        ))),
        Mode::PendingFactory => std::future::pending().await,
        Mode::PanicFactory => panic!("deliberate event factory unwind"),
        _ => Ok(()),
    }
}

pub(super) async fn resolve(probe: &Arc<Probe>, sequence: u64) -> chigiri::FieldResult<()> {
    let _lease = Lease {
        probe: Arc::clone(probe),
        phase: probe.event(sequence),
        stage: "resolver-drop",
    };
    match probe.mode {
        Mode::PendingResolver => std::future::pending().await,
        Mode::PanicResolver => panic!("deliberate event resolver unwind"),
        Mode::OwnedPanicResolver => {
            if let Some(release) = &probe.panic_release {
                release.notified().await;
            }
            std::panic::panic_any(Lease {
                probe: Arc::clone(probe),
                phase: probe.event(sequence),
                stage: "panic-payload-drop",
            })
        }
        Mode::ErrorFirstResolver if sequence == 1 => {
            Err(chigiri::FieldError::internal(PrivateError))
        }
        _ => Ok(()),
    }
}
