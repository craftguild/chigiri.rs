//! Bounded socket close and independent same-generation worker cleanup.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::super::{WebSocketConnectionCleanup, WebSocketConnectionClose};
use super::WebSocketIo;

pub(super) struct Closing {
    cleanup: WebSocketConnectionCleanup,
    deadline: BoxFuture<'static, ()>,
    close: Option<WebSocketConnectionClose>,
    socket_done: bool,
    cleanup_done: bool,
}

impl Closing {
    pub(super) fn new(
        cleanup: WebSocketConnectionCleanup,
        deadlines: &'static dyn DeadlineFactory,
        send_close: bool,
    ) -> Self {
        let close = send_close.then(|| cleanup.close());
        Self {
            cleanup,
            deadline: deadlines.sleep(Duration::from_secs(5)),
            close,
            socket_done: false,
            cleanup_done: false,
        }
    }

    pub(super) fn poll_socket<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
        context: &mut Context<'_>,
    ) -> Poll<()> {
        if self.deadline.as_mut().poll(context).is_ready() {
            // Driver Drop moves unfinished cleanup to its existing quarantine;
            // socket Drop releases only its own original physical references.
            return Poll::Ready(());
        }
        if !self.socket_done {
            self.socket_done = Pin::new(socket).poll_close(context, self.close).is_ready();
        }
        if self.socket_done {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    pub(super) fn poll_cleanup(
        &mut self,
        context: &mut Context<'_>,
        operations_done: bool,
    ) -> Poll<()> {
        if self.deadline.as_mut().poll(context).is_ready() {
            return Poll::Ready(());
        }
        if !self.cleanup_done {
            self.cleanup_done = Pin::new(&mut self.cleanup).poll(context).is_ready();
        }
        if self.cleanup_done && operations_done {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
