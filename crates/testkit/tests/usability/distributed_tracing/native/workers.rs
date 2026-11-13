//! Deliberately unrelated thread baselines exercise SDK restoration.

use std::cell::RefCell;

use opentelemetry::trace::TraceContextExt;
use opentelemetry::{Context, ContextGuard};

use super::{PrivateValue, parent};

thread_local! {
    static BASELINE: RefCell<Option<ContextGuard>> = const { RefCell::new(None) };
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .on_thread_start(|| {
            // This app-owned thread baseline is never stored in an SDK future.
            let context = Context::new()
                .with_remote_span_context(parent(9))
                .with_value(PrivateValue)
                .with_telemetry_suppressed();
            BASELINE.with(|slot| *slot.borrow_mut() = Some(context.attach()));
        })
        .on_thread_stop(|| BASELINE.with(|slot| drop(slot.borrow_mut().take())))
        .build()
        .unwrap()
}
