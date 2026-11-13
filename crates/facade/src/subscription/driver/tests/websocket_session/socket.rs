//! Controllable single-owner socket with a separate read-only test handle.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame,
    WebSocketWriteProgress,
};

pub(super) struct State {
    pub(super) input: VecDeque<WebSocketInbound>,
    pub(super) input_error: Option<WebSocketIoError>,
    pub(super) eof: bool,
    pub(super) current: Option<WebSocketWireFrame>,
    pub(super) retained: Option<WebSocketWireFrame>,
    pub(super) retain_next: bool,
    pub(super) automatic_flush: bool,
    pub(super) ready: bool,
    pub(super) progress: VecDeque<WebSocketWriteProgress>,
    pub(super) sent: Vec<String>,
    pub(super) closes: Vec<Option<u16>>,
    pub(super) on_receive: Option<Box<dyn FnOnce() + Send>>,
    pub(super) on_start: Option<Box<dyn FnOnce() + Send>>,
    reader: Option<Waker>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            input: VecDeque::new(),
            input_error: None,
            eof: false,
            current: None,
            retained: None,
            retain_next: false,
            automatic_flush: true,
            ready: true,
            progress: VecDeque::new(),
            sent: Vec::new(),
            closes: Vec::new(),
            on_receive: None,
            on_start: None,
            reader: None,
        }
    }
}

pub(in crate::subscription::driver::tests) struct Socket(Arc<Mutex<State>>);

#[derive(Clone, Default)]
pub(in crate::subscription::driver::tests) struct Handle(pub(super) Arc<Mutex<State>>);

impl Handle {
    pub(in crate::subscription::driver::tests) fn stall_flush(&self) {
        self.0.lock().unwrap().automatic_flush = false;
    }

    pub(in crate::subscription::driver::tests) fn socket(&self) -> Socket {
        Socket(Arc::clone(&self.0))
    }

    pub(in crate::subscription::driver::tests) fn text(&self, text: impl Into<String>) {
        let wake = {
            let mut state = self.0.lock().unwrap();
            state.input.push_back(WebSocketInbound::Text(text.into()));
            state.reader.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    pub(in crate::subscription::driver::tests) fn subscribe(&self, id: &str, query: &str) {
        // These explicit fixture inputs contain no characters requiring JSON
        // escaping. This is not a production encoder or untrusted-input parser.
        assert!(id.is_ascii() && !id.contains(['"', '\\']));
        assert!(query.is_ascii() && !query.contains(['"', '\\']));
        self.text(format!(
            r#"{{"id":"{id}","type":"subscribe","payload":{{"query":"{query}"}}}}"#
        ));
    }

    pub(in crate::subscription::driver::tests) fn messages(&self) -> Vec<String> {
        self.0.lock().unwrap().sent.clone()
    }

    pub(in crate::subscription::driver::tests) fn closes(&self) -> Vec<Option<u16>> {
        self.0.lock().unwrap().closes.clone()
    }

    pub(in crate::subscription::driver::tests) fn has_current_frame(&self) -> bool {
        self.0.lock().unwrap().current.is_some()
    }

    pub(super) fn current_len(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .as_bytes()
            .len()
    }
}

impl WebSocketIo for Socket {
    fn poll_receive(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        let replacement = context.waker().clone();
        let mut state = self.0.lock().unwrap();
        if let Some(message) = state.input.pop_front() {
            let action = state.on_receive.take();
            drop(state);
            if let Some(action) = action {
                action();
            }
            return Poll::Ready(Ok(Some(message)));
        }
        if let Some(error) = state.input_error.take() {
            return Poll::Ready(Err(error));
        }
        if state.eof {
            Poll::Ready(Ok(None))
        } else {
            let previous = state.reader.replace(replacement);
            drop(state);
            drop(previous);
            Poll::Pending
        }
    }

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WebSocketIoError>> {
        let state = self.0.lock().unwrap();
        assert!(
            state.current.is_none(),
            "the root must finish its sole physical frame first"
        );
        if state.ready {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn start_send(self: Pin<&mut Self>, frame: WebSocketWireFrame) -> Result<(), WebSocketIoError> {
        let action = self.0.lock().unwrap().on_start.take();
        if let Some(action) = action {
            action();
        }
        let mut state = self.0.lock().unwrap();
        assert!(state.current.is_none());
        state
            .sent
            .push(std::str::from_utf8(frame.as_bytes()).unwrap().to_owned());
        if state.retain_next {
            assert!(state.retained.is_none());
            state.retained = Some(frame.clone());
            state.retain_next = false;
        }
        state.current = Some(frame);
        Ok(())
    }

    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        let (progress, dropped) = {
            let mut state = self.0.lock().unwrap();
            let progress = match state.progress.pop_front() {
                Some(progress) => progress,
                None if state.automatic_flush => WebSocketWriteProgress {
                    accepted: state.current.as_ref().unwrap().as_bytes().len(),
                    flushed: true,
                },
                None => return Poll::Pending,
            };
            let dropped = if progress.flushed {
                state.current.take()
            } else {
                None
            };
            (progress, dropped)
        };
        drop(dropped);
        Poll::Ready(Ok(progress))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        let current = {
            let mut state = self.0.lock().unwrap();
            state.closes.push(close.map(WebSocketConnectionClose::code));
            state.current.take()
        };
        drop(current);
        Poll::Ready(Ok(()))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let (current, reader) = {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (state.current.take(), state.reader.take())
        };
        drop(current);
        drop(reader);
    }
}
