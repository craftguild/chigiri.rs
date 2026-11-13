//! Cancellation-safe lifecycle observation shared by operation execution seams.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::{
    LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult, OperationIdentity,
    RequestIdentity, RequestTelemetry,
};

pub(super) struct AdmissionLifecycle<'a> {
    observer: Option<&'a dyn LifecycleObserver>,
    identity: &'a OperationIdentity,
    started: Instant,
    completed: &'a AtomicBool,
    telemetry: Option<&'a RequestTelemetry>,
}

impl<'a> AdmissionLifecycle<'a> {
    pub(super) fn start(
        observer: Option<&'a dyn LifecycleObserver>,
        identity: &'a OperationIdentity,
        completed: &'a AtomicBool,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> Self {
        let started = Instant::now();
        observe(
            observer,
            LifecyclePhase::OperationAdmission,
            LifecycleResult::Started,
            None,
            identity,
            telemetry,
        );
        Self {
            observer,
            identity,
            started,
            completed,
            telemetry,
        }
    }

    pub(super) fn succeed(&self) {
        observe(
            self.observer,
            LifecyclePhase::OperationAdmission,
            LifecycleResult::Succeeded,
            Some(self.started.elapsed()),
            self.identity,
            self.telemetry,
        );
        self.completed.store(true, Ordering::SeqCst);
    }

    pub(super) fn reject(&self) {
        observe(
            self.observer,
            LifecyclePhase::OperationAdmission,
            LifecycleResult::Rejected,
            Some(self.started.elapsed()),
            self.identity,
            self.telemetry,
        );
        self.completed.store(true, Ordering::SeqCst);
    }
}

impl Drop for AdmissionLifecycle<'_> {
    fn drop(&mut self) {
        if !self.completed.load(Ordering::SeqCst) {
            observe(
                self.observer,
                LifecyclePhase::OperationAdmission,
                LifecycleResult::Cancelled,
                Some(self.started.elapsed()),
                self.identity,
                self.telemetry,
            );
        }
    }
}

pub(super) struct PhaseLifecycle<'a> {
    observer: Option<&'a dyn LifecycleObserver>,
    phase: LifecyclePhase,
    identity: &'a OperationIdentity,
    started: Instant,
    completed: bool,
    telemetry: Option<&'a RequestTelemetry>,
}

impl<'a> PhaseLifecycle<'a> {
    pub(super) fn start(
        observer: Option<&'a dyn LifecycleObserver>,
        phase: LifecyclePhase,
        identity: &'a OperationIdentity,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> Self {
        observe(
            observer,
            phase,
            LifecycleResult::Started,
            None,
            identity,
            telemetry,
        );
        Self {
            observer,
            phase,
            identity,
            started: Instant::now(),
            completed: false,
            telemetry,
        }
    }

    pub(super) fn finish(mut self, result: LifecycleResult) {
        observe(
            self.observer,
            self.phase,
            result,
            Some(self.started.elapsed()),
            self.identity,
            self.telemetry,
        );
        self.completed = true;
    }
}

impl Drop for PhaseLifecycle<'_> {
    fn drop(&mut self) {
        if !self.completed {
            observe(
                self.observer,
                self.phase,
                LifecycleResult::Cancelled,
                Some(self.started.elapsed()),
                self.identity,
                self.telemetry,
            );
        }
    }
}

fn observe(
    observer: Option<&dyn LifecycleObserver>,
    phase: LifecyclePhase,
    result: LifecycleResult,
    duration: Option<std::time::Duration>,
    identity: &OperationIdentity,
    telemetry: Option<&RequestTelemetry>,
) {
    if let Some(observer) = observer {
        observer.observe(LifecycleEvent::new(
            phase,
            result,
            duration,
            telemetry.map_or_else(
                || RequestIdentity::new(None, None, Some(identity)),
                |telemetry| telemetry.identity(Some(identity)),
            ),
            None,
            "chigiri",
        ));
    }
}
