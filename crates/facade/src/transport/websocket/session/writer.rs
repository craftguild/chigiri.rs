//! Sole physical opening-frame progress and successful acknowledgement proof.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::super::WebSocketAcknowledgementProgress;
use super::release::WebSocketWireRelease;
use super::{WebSocketIo, WebSocketIoError, WebSocketWireFrame};

pub(super) struct OpeningWrite {
    remaining: usize,
    acknowledgement: Option<WebSocketAcknowledgementProgress>,
    deadline: BoxFuture<'static, ()>,
    pending: Option<WebSocketWireFrame>,
    release: WebSocketWireRelease,
    flushed: bool,
}

impl OpeningWrite {
    pub(super) fn begin(
        frame: WebSocketWireFrame,
        acknowledgement: Option<WebSocketAcknowledgementProgress>,
        deadlines: &'static dyn DeadlineFactory,
        timeout: Duration,
    ) -> Self {
        let remaining = frame.as_bytes().len();
        let deadline = deadlines.sleep(timeout);
        let release = frame.release();
        Self {
            remaining,
            acknowledgement,
            deadline,
            pending: Some(frame),
            release,
            flushed: false,
        }
    }

    pub(super) fn poll<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
        context: &mut Context<'_>,
        deadlines: &'static dyn DeadlineFactory,
        timeout: Duration,
    ) -> Poll<Result<bool, WebSocketIoError>> {
        self.poll_scoped(socket, context, deadlines, timeout, None)
    }

    pub(super) fn poll_opening<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
        context: &mut Context<'_>,
        deadlines: &'static dyn DeadlineFactory,
        timeout: Duration,
        activity: &crate::managed::EventActivity,
    ) -> Poll<Result<bool, WebSocketIoError>> {
        self.poll_scoped(socket, context, deadlines, timeout, Some(activity))
    }

    fn poll_scoped<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
        context: &mut Context<'_>,
        deadlines: &'static dyn DeadlineFactory,
        timeout: Duration,
        opening: Option<&crate::managed::EventActivity>,
    ) -> Poll<Result<bool, WebSocketIoError>> {
        check_opening(opening)?;
        if self.flushed {
            // All payload bytes were accepted. Physical retention is not a stall.
            return self
                .release
                .poll(context)
                .map(|result| result.map(|()| true));
        }
        // The original timer wins a simultaneous progress/expiry race.
        if self.deadline.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(WebSocketIoError::Transport));
        }
        if self.pending.is_some() {
            match Pin::new(&mut *socket).poll_ready(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => {}
            }
            if let Err(error) = self.start_ready(socket, context, opening) {
                return Poll::Ready(Err(error));
            }
        }
        check_opening(opening)?;
        let progress = match Pin::new(&mut *socket).poll_write(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(progress)) => progress,
        };
        check_opening(opening)?;
        if progress.accepted > self.remaining || (progress.accepted == 0 && !progress.flushed) {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        }
        self.remaining -= progress.accepted;
        if progress.flushed {
            if self.remaining != 0 {
                return Poll::Ready(Err(WebSocketIoError::Invariant));
            }
            if let Some(proof) = self.acknowledgement.take() {
                proof.written_and_flushed();
            }
            self.flushed = true;
            return self
                .release
                .poll(context)
                .map(|result| result.map(|()| true));
        }
        if progress.accepted > 0 {
            self.deadline = deadlines.sleep(timeout);
        }
        Poll::Ready(Ok(false))
    }

    /// Starts within the same exclusive interval as the caller's readiness and
    /// generation fence. A terminal control must not wait in a pre-start owner
    /// across another input poll that can accept a replacement operation.
    pub(super) fn start_ready<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
        context: &mut Context<'_>,
        opening: Option<&crate::managed::EventActivity>,
    ) -> Result<(), WebSocketIoError> {
        self.check_start(context)?;
        check_opening(opening)?;
        self.start_authorized(socket)
    }

    pub(super) fn check_start(
        &mut self,
        context: &mut Context<'_>,
    ) -> Result<(), WebSocketIoError> {
        if self.deadline.as_mut().poll(context).is_ready() {
            return Err(WebSocketIoError::Transport);
        }
        Ok(())
    }

    /// Contains no timer poll between the caller's permission and native send.
    pub(super) fn start_authorized<S: WebSocketIo>(
        &mut self,
        socket: &mut S,
    ) -> Result<(), WebSocketIoError> {
        let frame = self.pending.take().ok_or(WebSocketIoError::Invariant)?;
        Pin::new(socket).start_send(frame)
    }
}

fn check_opening(activity: Option<&crate::managed::EventActivity>) -> Result<(), WebSocketIoError> {
    if activity.is_some_and(crate::managed::EventActivity::is_stopping) {
        Err(WebSocketIoError::Transport)
    } else {
        Ok(())
    }
}
