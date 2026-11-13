//! Application-owned request data and the documentation's compiled factory recipes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) static PENDING_DROPS: AtomicUsize = AtomicUsize::new(0);
pub(crate) static CONTEXT_DROPS: AtomicUsize = AtomicUsize::new(0);
static NEXT: AtomicUsize = AtomicUsize::new(1);

pub(crate) struct AppContext {
    pub(crate) users: Arc<domain::Users>,
    pub(crate) id: usize,
    pub(crate) session: String,
    pub(crate) cache: Mutex<Vec<String>>,
}

impl Drop for AppContext {
    fn drop(&mut self) {
        CONTEXT_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

impl AppContext {
    async fn prepare(
        request: chigiri::HttpRequestContext<'_>,
    ) -> Result<Self, chigiri::PrepareError> {
        Self::with_users(request, Arc::new(domain::Users)).await
    }

    async fn with_users(
        request: chigiri::HttpRequestContext<'_>,
        users: Arc<domain::Users>,
    ) -> Result<Self, chigiri::PrepareError> {
        match header(request, "x-case") {
            "reject" => {
                return Err(chigiri::PrepareError::Reject(
                    chigiri::spi::Rejection::new(
                        403,
                        chigiri::GraphQlError::new("Preparation rejected").code("DENIED"),
                    )
                    .unwrap(),
                ));
            }
            "internal" => {
                return Err(chigiri::PrepareError::Internal(chigiri::HookError::new(
                    std::io::Error::other("private preparation credential"),
                )));
            }
            "pending" => {
                let _guard = PendingGuard;
                std::future::pending::<()>().await;
            }
            _ => {}
        }
        Ok(Self {
            users,
            id: NEXT.fetch_add(1, Ordering::SeqCst),
            session: header(request, "x-session").to_owned(),
            cache: Mutex::new(Vec::new()),
        })
    }
}

fn header<'a>(request: chigiri::HttpRequestContext<'a>, name: &str) -> &'a str {
    request
        .transport()
        .headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .and_then(|h| std::str::from_utf8(h.value).ok())
        .unwrap_or_default()
}

struct PendingGuard;

impl Drop for PendingGuard {
    fn drop(&mut self) {
        PENDING_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

// The integration harness extracts these two functions from the maintained guide.
include!("recipes.rs");

#[path = "http.rs"]
mod tests;
