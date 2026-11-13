//! Axum body adaptation that preserves the common physical byte owner.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use chigiri::__streaming::{HttpStreamingBody, HttpStreamingError, SubscriptionInitialResponse};
use futures_core::Stream;
use http_body_util::{BodyExt, Empty};

pub(super) struct AxumStreamingBody {
    inner: HttpStreamingBody,
    done: bool,
}

impl AxumStreamingBody {
    pub(super) const fn new(inner: HttpStreamingBody) -> Self {
        Self { inner, done: false }
    }
}

impl Stream for AxumStreamingBody {
    type Item = Result<Bytes, HttpStreamingError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        match self.inner.poll_next(context) {
            Poll::Ready(Some(Ok(frame))) => match frame.accept() {
                Ok(frame) => Poll::Ready(Some(Ok(owner_bytes(frame)))),
                Err(error) => {
                    self.done = true;
                    Poll::Ready(Some(Err(error)))
                }
            },
            Poll::Ready(Some(Err(error))) => {
                self.done = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(super) fn prepared_finite_body(response: SubscriptionInitialResponse) -> Body {
    finite_owner_body(response)
}

pub(super) fn prepared_streaming_body(response: SubscriptionInitialResponse) -> Body {
    Body::from_stream(PreparedOwnerStream::new(response))
}

fn finite_owner_body<T>(owner: T) -> Body
where
    T: AsRef<[u8]> + Send + 'static,
{
    if owner.as_ref().is_empty() {
        let guarded = Empty::<Bytes>::new().map_err(move |error: Infallible| {
            let _owner = &owner;
            error
        });
        Body::new(guarded)
    } else {
        Body::from(owner_bytes(owner))
    }
}

struct PreparedOwnerStream<T> {
    owner: Option<T>,
}

impl<T> PreparedOwnerStream<T> {
    const fn new(owner: T) -> Self {
        Self { owner: Some(owner) }
    }
}

impl<T> Stream for PreparedOwnerStream<T>
where
    T: AsRef<[u8]> + Send + Unpin + 'static,
{
    type Item = Result<Bytes, Infallible>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let Some(owner) = this.owner.as_ref() else {
            return Poll::Ready(None);
        };
        if owner.as_ref().is_empty() {
            return Poll::Ready(None);
        }
        let owner = this.owner.take().expect("checked prepared response owner");
        Poll::Ready(Some(Ok(owner_bytes(owner))))
    }
}

fn owner_bytes<T>(owner: T) -> Bytes
where
    T: AsRef<[u8]> + Send + 'static,
{
    Bytes::from_owner(owner)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::HttpBody;

    use super::*;

    struct TrackedOwner {
        drops: Arc<AtomicUsize>,
        bytes: &'static [u8],
    }

    impl AsRef<[u8]> for TrackedOwner {
        fn as_ref(&self) -> &[u8] {
            self.bytes
        }
    }

    impl Drop for TrackedOwner {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[test]
    fn byte_clones_retain_the_original_physical_owner() {
        let drops = Arc::new(AtomicUsize::new(0));
        let bytes = owner_bytes(TrackedOwner {
            drops: Arc::clone(&drops),
            bytes: b"frame",
        });
        let clone = bytes.clone();

        drop(bytes);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        assert_eq!(&clone[..], b"frame");
        drop(clone);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn finite_empty_body_retains_owner_and_has_exact_zero_hint() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut body = finite_owner_body(TrackedOwner {
            drops: Arc::clone(&drops),
            bytes: b"",
        });

        assert_eq!(body.size_hint().exact(), Some(0));
        assert!(body.is_end_stream());
        assert!(BodyExt::frame(&mut body).await.is_none());
        assert_eq!(drops.load(Ordering::Acquire), 0);
        drop(body);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn streaming_empty_body_retains_owner_and_has_unknown_hint() {
        let drops = Arc::new(AtomicUsize::new(0));
        let stream = PreparedOwnerStream::new(TrackedOwner {
            drops: Arc::clone(&drops),
            bytes: b"",
        });
        let mut body = Body::from_stream(stream);

        assert_eq!(body.size_hint().exact(), None);
        assert!(!body.is_end_stream());
        assert!(BodyExt::frame(&mut body).await.is_none());
        assert_eq!(drops.load(Ordering::Acquire), 0);
        drop(body);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn prepared_nonempty_bytes_retain_owner_until_last_clone() {
        let drops = Arc::new(AtomicUsize::new(0));
        let stream = PreparedOwnerStream::new(TrackedOwner {
            drops: Arc::clone(&drops),
            bytes: b"frame",
        });
        let mut body = Body::from_stream(stream);

        assert_eq!(body.size_hint().exact(), None);
        assert!(!body.is_end_stream());
        let frame = BodyExt::frame(&mut body)
            .await
            .expect("prepared frame")
            .expect("infallible prepared frame");
        let bytes = frame.into_data().expect("data frame");
        let clone = bytes.clone();
        drop(body);
        drop(bytes);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        drop(clone);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn finite_nonempty_body_has_exact_hint_and_retains_cloned_bytes() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut body = finite_owner_body(TrackedOwner {
            drops: Arc::clone(&drops),
            bytes: b"frame",
        });

        assert_eq!(body.size_hint().exact(), Some(5));
        assert!(!body.is_end_stream());
        let frame = BodyExt::frame(&mut body)
            .await
            .expect("finite frame")
            .expect("infallible finite frame");
        let bytes = frame.into_data().expect("data frame");
        let clone = bytes.clone();
        drop(body);
        drop(bytes);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        drop(clone);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }
}
