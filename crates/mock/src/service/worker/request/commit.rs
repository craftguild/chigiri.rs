//! Keep complete request state and publication transfer within the same store win.

use super::{Request, State, environment};
use crate::luau::ScriptError;

impl Request {
    /// Retain candidates and retired ownership until all commit locks are released.
    pub fn commit(&self, logs: Option<(u64, &crate::logging::Logger)>) -> Result<(), ScriptError> {
        if *self.state.lock().expect("request state lock") == State::Committed {
            return Ok(());
        }
        let mut prepared = self.prepared.lock().expect("prepared request lock").take();
        let result = self
            .cancel
            .commit(&self.shutdown, self.deadline, || {
                let mut state = self.state.lock().expect("request state lock");
                if *state != State::Ready {
                    return Err(environment("request not ready"));
                }
                if self.rejections.any() {
                    return Err(environment("request rejected"));
                }
                let prepared = prepared
                    .as_mut()
                    .ok_or_else(|| environment("request preparation missing"))?;
                prepared
                    .store
                    .check()
                    .map_err(|_| environment("request budget failed"))?;
                let retired = if self.mutation {
                    Some(
                        self.committed
                            .lock()
                            .expect("store lock must be available")
                            .commit_publications(&prepared.store, &mut prepared.publications)
                            .map_err(|_| environment("request commit rejected"))?,
                    )
                } else {
                    None
                };
                *state = State::Committed;
                Ok(retired)
            })
            .unwrap_or_else(|| Err(environment("request commit cancelled")));
        if result.is_err() {
            if *self.state.lock().expect("request state lock") == State::Committed {
                return Ok(());
            }
            self.fail();
        }
        result.map(|retired| {
            // The cancellation/store/state locks have all ended. A logger
            // failure cannot change the already committed request outcome.
            if let (Some(retired), Some((id, logger))) = (retired, logs) {
                for decision in retired.observations() {
                    logger.publication(
                        id,
                        decision.sequence,
                        decision.index,
                        decision.coordinate,
                        decision.lost,
                    );
                }
            }
        })
    }
}
