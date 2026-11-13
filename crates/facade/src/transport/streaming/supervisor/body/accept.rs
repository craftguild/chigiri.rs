//! Exact managed emission acceptance and the last physical-frame release.

use super::{AcceptedHttpStreamingFrame, HttpStreamingError, HttpStreamingFrame};

impl HttpStreamingFrame {
    /// Records whole-entry acceptance at the exact body yield point.
    ///
    /// Move the result directly into the server's zero-copy byte owner. Its
    /// physical Drop notification is separate from this accepted-byte progress.
    pub fn accept(self) -> Result<AcceptedHttpStreamingFrame, HttpStreamingError> {
        let port = self.port.upgrade().ok_or(HttpStreamingError::Closed)?;
        let timeline = port
            .timeline
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        let control = port.control();
        let gate = control
            .as_ref()
            .map(|control| control.emission_gate())
            .transpose()
            .map_err(|_| HttpStreamingError::Closed)?;
        let now = port.clock.now()?;
        let mut state = port
            .state
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        if state.body_closed
            || state.eof
            || !state.live
            || state.generation != self.generation
            || state.accepted.is_some()
        {
            return Err(HttpStreamingError::Closed);
        }
        let length = self
            .bytes
            .as_ref()
            .ok_or(HttpStreamingError::Invariant)?
            .as_ref()
            .len();
        state.accepted = Some((length, now));
        let wake = state.driver.take();
        drop(state);
        drop(timeline);
        drop(gate);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(AcceptedHttpStreamingFrame { frame: self })
    }
}

impl Drop for HttpStreamingFrame {
    fn drop(&mut self) {
        drop(self.bytes.take());
        drop(self.physical.take());
        let Some(port) = self.port.upgrade() else {
            return;
        };
        let mut state = port
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.generation != self.generation {
            return;
        }
        state.live = false;
        state.released = true;
        let wake = state.driver.take();
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
