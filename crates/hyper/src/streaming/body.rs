//! Hyper body adaptation that preserves the common physical byte owner.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use chigiri::__streaming::{HttpStreamingBody, HttpStreamingError, SubscriptionInitialResponse};
use hyper::body::{Body, Frame, SizeHint};

/// A finite or supervised streaming Hyper response body.
///
/// Accepted streaming frames are moved into [`Bytes`] without copying. Their
/// common byte charge and physical lease therefore remain alive through every
/// clone until the final `Bytes` reference is dropped by Hyper.
pub struct HyperStreamingBody {
    state: BodyState,
}

enum BodyState {
    Once { body: Option<Bytes>, exact: bool },
    Streaming(HttpStreamingBody),
    Done,
}

impl HyperStreamingBody {
    pub(crate) fn finite(body: Vec<u8>) -> Self {
        let body = Bytes::from(body);
        Self {
            state: BodyState::Once {
                body: (!body.is_empty()).then_some(body),
                exact: true,
            },
        }
    }

    pub(super) fn prepared(response: SubscriptionInitialResponse) -> Self {
        let exact = !response.is_streaming_profile();
        Self {
            // Keep an empty owner until this body is dropped. The initial
            // response lease covers physical response ownership, not only bytes.
            state: BodyState::Once {
                body: Some(owned_bytes(response)),
                exact,
            },
        }
    }

    pub(super) const fn streaming(body: HttpStreamingBody) -> Self {
        Self {
            state: BodyState::Streaming(body),
        }
    }
}

impl Body for HyperStreamingBody {
    type Data = Bytes;
    type Error = HttpStreamingError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.as_mut().get_mut();
        match &mut this.state {
            BodyState::Once { body, .. } => {
                if body.as_ref().is_some_and(Bytes::is_empty) {
                    return Poll::Ready(None);
                }
                Poll::Ready(body.take().map(|body| Ok(Frame::data(body))))
            }
            BodyState::Streaming(body) => match body.poll_next(context) {
                Poll::Ready(Some(Ok(frame))) => match frame.accept() {
                    Ok(frame) => Poll::Ready(Some(Ok(Frame::data(owned_bytes(frame))))),
                    Err(error) => {
                        this.state = BodyState::Done;
                        Poll::Ready(Some(Err(error)))
                    }
                },
                Poll::Ready(Some(Err(error))) => {
                    this.state = BodyState::Done;
                    Poll::Ready(Some(Err(error)))
                }
                Poll::Ready(None) => {
                    this.state = BodyState::Done;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            },
            BodyState::Done => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.state {
            BodyState::Once { body, .. } => body.as_ref().is_none_or(Bytes::is_empty),
            BodyState::Streaming(_) => false,
            BodyState::Done => true,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.state {
            BodyState::Once {
                body: Some(body),
                exact: true,
            } => SizeHint::with_exact(body.len() as u64),
            BodyState::Once {
                body: None,
                exact: true,
            }
            | BodyState::Done => SizeHint::with_exact(0),
            BodyState::Once { exact: false, .. } | BodyState::Streaming(_) => SizeHint::default(),
        }
    }
}

fn owned_bytes<T>(owner: T) -> Bytes
where
    T: AsRef<[u8]> + Send + 'static,
{
    Bytes::from_owner(owner)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Waker;

    use super::*;

    struct Owner {
        bytes: Vec<u8>,
        drops: Arc<AtomicUsize>,
    }

    impl AsRef<[u8]> for Owner {
        fn as_ref(&self) -> &[u8] {
            &self.bytes
        }
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn finite_body_has_an_exact_size_and_one_frame() {
        let mut body = HyperStreamingBody::finite(b"finite".to_vec());
        assert_eq!(body.size_hint().exact(), Some(6));

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let Poll::Ready(Some(Ok(frame))) = Pin::new(&mut body).poll_frame(&mut context) else {
            panic!("finite body must return one data frame");
        };
        let frame = frame.into_data().expect("data frame");
        assert_eq!(frame, Bytes::from_static(b"finite"));
        assert!(body.is_end_stream());
        assert!(matches!(
            Pin::new(&mut body).poll_frame(&mut context),
            Poll::Ready(None)
        ));
    }

    #[test]
    fn empty_finite_body_is_already_complete() {
        let mut body = HyperStreamingBody::finite(Vec::new());
        assert_eq!(body.size_hint().exact(), Some(0));
        assert!(body.is_end_stream());

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            Pin::new(&mut body).poll_frame(&mut context),
            Poll::Ready(None)
        ));
    }

    #[test]
    fn empty_prepared_owner_survives_end_stream_poll_until_body_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut body = HyperStreamingBody {
            state: BodyState::Once {
                body: Some(owned_bytes(Owner {
                    bytes: Vec::new(),
                    drops: Arc::clone(&drops),
                })),
                exact: true,
            },
        };
        assert!(body.is_end_stream());

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            Pin::new(&mut body).poll_frame(&mut context),
            Poll::Ready(None)
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        drop(body);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn selected_profile_owner_has_no_exact_size_hint() {
        let drops = Arc::new(AtomicUsize::new(0));
        let body = HyperStreamingBody {
            state: BodyState::Once {
                body: Some(owned_bytes(Owner {
                    bytes: b"selected error".to_vec(),
                    drops: Arc::clone(&drops),
                })),
                exact: false,
            },
        };

        assert_eq!(body.size_hint().exact(), None);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(body);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn physical_owner_survives_every_bytes_clone() {
        let drops = Arc::new(AtomicUsize::new(0));
        let bytes = owned_bytes(Owner {
            bytes: b"owned".to_vec(),
            drops: Arc::clone(&drops),
        });
        let last = bytes.clone();
        drop(bytes);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(last.as_ref(), b"owned");
        drop(last);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
