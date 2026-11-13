//! Actual socket initialization through authorization and physical ACK release.

mod poll;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::DeadlineFactory;
use chigiri_runtime::instrumentation::InstrumentationScope;

use super::super::{
    AckPendingWebSocketConnection, InitializedWebSocketConnection,
    WebSocketAcknowledgementResolution, WebSocketConnectionClose, WebSocketConnectionHandoff,
    WebSocketConnectionHandoffResolution, WebSocketConnectionInitialization,
    WebSocketConnectionResolution,
};
use super::closing::Closing;
use super::writer::OpeningWrite;
use super::{WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame};

const POLL_WORK: usize = 32;

enum Opening {
    Handoff(Box<WebSocketConnectionHandoff>),
    Initializing(Box<WebSocketConnectionInitialization>),
    Acknowledging(AckPendingWebSocketConnection),
    Closing(Closing),
    Finished,
}

/// One application-driven socket owner before GraphQL operation admission.
///
/// This is the opening prefix of the common session, not a spawned task. The
/// same socket and same connection owner move to operation delivery on success.
#[must_use = "the socket opening driver must be polled or explicitly dropped"]
#[doc(hidden)]
pub struct WebSocketOpeningDriver<S: WebSocketIo> {
    socket: Option<S>,
    opening: Opening,
    write: Option<OpeningWrite>,
    deadlines: &'static dyn DeadlineFactory,
    send_timeout: Duration,
    send_close: bool,
    activity: crate::managed::EventActivity,
    instrumentation: Option<InstrumentationScope>,
}

/// The opening result; success alone does not prove a complete protocol session.
#[doc(hidden)]
pub enum WebSocketOpeningOutcome<S> {
    /// Authorization, successful ACK flush, and last ACK physical Drop completed.
    Acknowledged {
        /// The original application-owned socket, without a replacement stream.
        socket: S,
        /// The same charged connection that was admitted before HTTP 101.
        connection: InitializedWebSocketConnection,
    },
    /// The socket was closed and cleanup completed or entered charged quarantine.
    Closed,
}

impl<S: WebSocketIo> WebSocketOpeningDriver<S> {
    pub(crate) fn new(
        handoff: WebSocketConnectionHandoff,
        socket: S,
        deadlines: &'static dyn DeadlineFactory,
        send_timeout: Duration,
    ) -> Self {
        let activity = handoff.activity();
        let instrumentation = handoff.instrumentation();
        Self {
            socket: Some(socket),
            opening: Opening::Handoff(Box::new(handoff)),
            write: None,
            deadlines,
            send_timeout,
            send_close: true,
            activity,
            instrumentation,
        }
    }

    pub(super) fn instrumentation(&self) -> Option<InstrumentationScope> {
        self.instrumentation.clone()
    }

    fn stop(&mut self) {
        // Native buffers finish destruction before the original connection
        // state enters charged recovery. This never starts another I/O poll.
        let owned = (
            self.socket.take(),
            std::mem::replace(&mut self.opening, Opening::Finished),
            self.write.take(),
        );
        drop(owned);
    }

    fn close(&mut self, close: WebSocketConnectionClose, send_close: bool) {
        self.send_close &= send_close;
        self.write.take();
        match &mut self.opening {
            Opening::Initializing(initialization) => {
                initialization.request_close(close);
            }
            Opening::Acknowledging(acknowledgement) => {
                acknowledgement.request_close(close);
            }
            _ => {}
        }
    }

    fn io_failure(&mut self, error: WebSocketIoError) {
        let (close, send) = match error {
            WebSocketIoError::Protocol => (WebSocketConnectionClose::bad_request(), true),
            WebSocketIoError::Transport | WebSocketIoError::Invariant => {
                (WebSocketConnectionClose::internal(), false)
            }
        };
        self.close(close, send);
    }

    fn poll_phase(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Option<InitializedWebSocketConnection>> {
        let next = match &mut self.opening {
            Opening::Handoff(handoff) => match Pin::new(handoff).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(WebSocketConnectionHandoffResolution::Initializing(initialization)) => {
                    Opening::Initializing(initialization)
                }
                Poll::Ready(WebSocketConnectionHandoffResolution::Closing(cleanup)) => {
                    Opening::Closing(Closing::new(cleanup, self.deadlines, self.send_close))
                }
            },
            Opening::Initializing(initialization) => match Pin::new(initialization).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(WebSocketConnectionResolution::Acknowledgement(acknowledgement)) => {
                    Opening::Acknowledging(acknowledgement)
                }
                Poll::Ready(WebSocketConnectionResolution::Closing(cleanup)) => {
                    Opening::Closing(Closing::new(cleanup, self.deadlines, self.send_close))
                }
            },
            Opening::Acknowledging(acknowledgement) => {
                match Pin::new(acknowledgement).poll(context) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(WebSocketAcknowledgementResolution::Initialized(connection)) => {
                        // The ACK gate proves flush and physical payload release.
                        // Only a payload-free release waiter may still remain.
                        self.write.take();
                        self.opening = Opening::Finished;
                        return Poll::Ready(Some(connection));
                    }
                    Poll::Ready(WebSocketAcknowledgementResolution::Closing(cleanup)) => {
                        Opening::Closing(Closing::new(cleanup, self.deadlines, self.send_close))
                    }
                }
            }
            Opening::Closing(_) | Opening::Finished => return Poll::Pending,
        };
        self.opening = next;
        Poll::Ready(None)
    }

    fn poll_input(&mut self, context: &mut Context<'_>) -> bool {
        let socket = self.socket.as_mut().expect("live opening socket");
        let message = match Pin::new(socket).poll_receive(context) {
            Poll::Pending => return false,
            Poll::Ready(Err(error)) => {
                self.io_failure(error);
                return true;
            }
            Poll::Ready(Ok(None)) => {
                self.close(WebSocketConnectionClose::internal(), false);
                return true;
            }
            Poll::Ready(Ok(Some(message))) => message,
        };
        if self.activity.is_stopping() {
            drop(message);
            self.stop();
            return true;
        }
        match (&mut self.opening, message) {
            (Opening::Initializing(initialization), WebSocketInbound::Text(text)) => {
                let _ = initialization.receive_text(&text);
            }
            (Opening::Acknowledging(acknowledgement), WebSocketInbound::Text(text)) => {
                let _ = acknowledgement.receive_text(&text);
            }
            (Opening::Initializing(initialization), WebSocketInbound::Binary) => {
                initialization.receive_binary();
            }
            (Opening::Acknowledging(acknowledgement), WebSocketInbound::Binary) => {
                acknowledgement.receive_binary();
            }
            _ => unreachable!("only a live initialization phase polls input"),
        }
        true
    }

    fn poll_writer(&mut self, context: &mut Context<'_>) -> bool {
        if let Some(write) = &mut self.write {
            let result = write.poll_opening(
                self.socket.as_mut().expect("live opening socket"),
                context,
                self.deadlines,
                self.send_timeout,
                &self.activity,
            );
            return match result {
                Poll::Pending => false,
                Poll::Ready(Ok(flushed)) => {
                    if flushed {
                        self.write.take();
                    }
                    true
                }
                Poll::Ready(Err(error)) => {
                    self.io_failure(error);
                    true
                }
            };
        }
        let next = match &mut self.opening {
            Opening::Initializing(initialization) => initialization
                .begin_control_send()
                .map(|frame| frame.map(|frame| (WebSocketWireFrame::control(frame), None))),
            Opening::Acknowledging(acknowledgement) => {
                acknowledgement.begin_acknowledgement().map(|frame| {
                    frame.map(|(frame, proof)| {
                        (WebSocketWireFrame::acknowledgement(frame), Some(proof))
                    })
                })
            }
            _ => return false,
        };
        match next {
            Ok(Some((frame, proof))) => {
                self.write = Some(OpeningWrite::begin(
                    frame,
                    proof,
                    self.deadlines,
                    self.send_timeout,
                ));
                true
            }
            Ok(None) => false,
            Err(close) => {
                self.close(close, true);
                true
            }
        }
    }
}
