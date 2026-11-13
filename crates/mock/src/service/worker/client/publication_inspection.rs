//! Observe real host fan-out completion without driving or draining publications.

use super::super::Client;

impl Client {
    pub(in crate::service) fn publication_stage_idle_for_test(&self) -> bool {
        crate::publication::test_support::stage_idle(&self.committed.lock().unwrap())
    }
}
