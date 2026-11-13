//! Verify response hooks use the finalized typed component graph.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::injection_support::execute_http;
use super::support::resolvers;

#[test]
fn response_hook_convenience_and_explicit_injection_return_the_same_response() {
    let sugar_calls = Arc::new(AtomicUsize::new(0));
    let inject_calls = Arc::new(AtomicUsize::new(0));
    let sugar = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .response_hook(HeaderHook::new("same", Arc::clone(&sugar_calls)))
        .body_limit(4_096)
        .build()
        .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .inject(|components| {
            components
                .http()
                .response_hook(HeaderHook::new("same", Arc::clone(&inject_calls)))
                .body_limit(4_096);
        })
        .build()
        .unwrap();
    assert_eq!(execute_http(&sugar), execute_http(&injected));
    assert_eq!(sugar_calls.load(Ordering::SeqCst), 1);
    assert_eq!(inject_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn only_the_last_injected_response_hook_runs() {
    let first = Arc::new(AtomicUsize::new(0));
    let middle = Arc::new(AtomicUsize::new(0));
    let last = Arc::new(AtomicUsize::new(0));
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .response_hook(HeaderHook::new("first", Arc::clone(&first)))
        .body_limit(2_048)
        .inject(|components| {
            components
                .http()
                .response_hook(HeaderHook::new("middle", Arc::clone(&middle)));
        })
        .execution_timeout(Duration::from_secs(2))
        .response_hook(HeaderHook::new("last", Arc::clone(&last)))
        .build()
        .unwrap();
    let response = execute_http(&endpoint);
    assert_eq!(first.load(Ordering::SeqCst), 0);
    assert_eq!(middle.load(Ordering::SeqCst), 0);
    assert_eq!(last.load(Ordering::SeqCst), 1);
    assert!(response.headers.iter().any(|header| header.value == "last"));
}

struct HeaderHook {
    value: &'static str,
    calls: Arc<AtomicUsize>,
}

impl HeaderHook {
    fn new(value: &'static str, calls: Arc<AtomicUsize>) -> Self {
        Self { value, calls }
    }
}

impl chigiri::ResponseHook<()> for HeaderHook {
    fn call<'a>(
        &'a self,
        _: chigiri::ResponseHookContext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::ResponseHeaders, chigiri::HookError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let mut headers = chigiri::ResponseHeaders::new();
            headers
                .append("x-component-hook", self.value)
                .map_err(chigiri::HookError::new)?;
            Ok(headers)
        })
    }
}
