//! Opening poll and destruction always restore the caller's ambient context.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{Opening, POLL_WORK, WebSocketIo, WebSocketOpeningDriver, WebSocketOpeningOutcome};

impl<S: WebSocketIo> WebSocketOpeningDriver<S> {
    fn poll_owned(&mut self, context: &mut Context<'_>) -> Poll<WebSocketOpeningOutcome<S>> {
        for _ in 0..POLL_WORK {
            let this = &mut *self;
            if this.activity.is_stopping() {
                this.stop();
                return Poll::Ready(WebSocketOpeningOutcome::Closed);
            }
            if let Opening::Closing(closing) = &mut this.opening {
                if let Some(socket) = this.socket.as_mut() {
                    if closing.poll_socket(socket, context).is_pending() {
                        return Poll::Pending;
                    }
                    this.socket.take();
                }
                if closing.poll_cleanup(context, true).is_pending() {
                    return Poll::Pending;
                }
                this.opening = Opening::Finished;
                return Poll::Ready(WebSocketOpeningOutcome::Closed);
            }
            match this.poll_phase(context) {
                Poll::Ready(Some(connection)) => {
                    if this.activity.is_stopping() {
                        this.stop();
                        drop(connection);
                        return Poll::Ready(WebSocketOpeningOutcome::Closed);
                    }
                    debug_assert!(this.write.is_none());
                    return Poll::Ready(WebSocketOpeningOutcome::Acknowledged {
                        socket: this.socket.take().expect("acknowledged socket"),
                        connection,
                    });
                }
                Poll::Ready(None) => continue,
                Poll::Pending => {}
            }
            let input = this.poll_input(context);
            // Recheck a protocol terminal before taking any additional frame.
            if input {
                match this.poll_phase(context) {
                    Poll::Ready(Some(connection)) => {
                        if this.activity.is_stopping() {
                            this.stop();
                            drop(connection);
                            return Poll::Ready(WebSocketOpeningOutcome::Closed);
                        }
                        return Poll::Ready(WebSocketOpeningOutcome::Acknowledged {
                            socket: this.socket.take().expect("acknowledged socket"),
                            connection,
                        });
                    }
                    Poll::Ready(None) => continue,
                    Poll::Pending => {}
                }
            }
            let output = this.poll_writer(context);
            if !input && !output {
                return Poll::Pending;
            }
        }
        context.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<S: WebSocketIo> Future for WebSocketOpeningDriver<S> {
    type Output = WebSocketOpeningOutcome<S>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.instrumentation.clone() {
            Some(scope) => scope.run(|| this.poll_owned(context)),
            None => this.poll_owned(context),
        }
    }
}

impl<S: WebSocketIo> Drop for WebSocketOpeningDriver<S> {
    fn drop(&mut self) {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| self.stop()),
            None => self.stop(),
        }
    }
}
