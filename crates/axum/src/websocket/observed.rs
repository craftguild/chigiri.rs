use std::io::{self, IoSlice};
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::WebSocketWriteProgressTracker;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(super) struct ObservedIo<S> {
    inner: S,
    tracker: WebSocketWriteProgressTracker,
    pending_application_bytes: usize,
    failed: bool,
}

impl<S> ObservedIo<S> {
    pub(super) const fn new(inner: S) -> Self {
        Self {
            inner,
            tracker: WebSocketWriteProgressTracker::new(),
            pending_application_bytes: 0,
            failed: false,
        }
    }

    pub(super) fn arm_text(&mut self, payload_bytes: usize) -> io::Result<()> {
        self.tracker
            .arm_text(payload_bytes)
            .map_err(|_| self.observation_error())
    }

    pub(super) fn finish_text_flush(&mut self) -> io::Result<()> {
        self.tracker
            .finish_flush()
            .map_err(|_| self.observation_error())
    }

    pub(super) const fn failed(&self) -> bool {
        self.failed
    }

    pub(super) fn take_application_bytes(&mut self) -> usize {
        std::mem::take(&mut self.pending_application_bytes)
    }

    fn observe_scalar(&mut self, bytes: &[u8], accepted: usize) -> io::Result<()> {
        if accepted > bytes.len() {
            return Err(self.observation_error());
        }
        let progress = self
            .tracker
            .observe_accepted(&bytes[..accepted])
            .map_err(|_| self.observation_error())?;
        self.add_progress(progress.application_bytes())
    }

    fn observe_vectored(&mut self, bytes: &[IoSlice<'_>], accepted: usize) -> io::Result<()> {
        let progress = self
            .tracker
            .observe_accepted_vectored(bytes, accepted)
            .map_err(|_| self.observation_error())?;
        self.add_progress(progress.application_bytes())
    }

    fn add_progress(&mut self, accepted: usize) -> io::Result<()> {
        self.pending_application_bytes = self
            .pending_application_bytes
            .checked_add(accepted)
            .ok_or_else(|| self.observation_error())?;
        Ok(())
    }

    fn observation_error(&mut self) -> io::Error {
        self.failed = true;
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid passive WebSocket write observation",
        )
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ObservedIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for ObservedIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(context, bytes) {
            Poll::Ready(Ok(accepted)) => match this.observe_scalar(bytes, accepted) {
                Ok(()) => Poll::Ready(Ok(accepted)),
                Err(error) => Poll::Ready(Err(error)),
            },
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write_vectored(context, buffers) {
            Poll::Ready(Ok(accepted)) => match this.observe_vectored(buffers, accepted) {
                Ok(()) => Poll::Ready(Ok(accepted)),
                Err(error) => Poll::Ready(Err(error)),
            },
            other => other,
        }
    }
}
