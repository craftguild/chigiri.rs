//! One fixed monotonic epoch for connection event accounting.

pub(super) struct SessionClock {
    epoch: std::time::Instant,
    #[cfg(test)]
    manual: Option<u64>,
    #[cfg(test)]
    failed: bool,
}

impl SessionClock {
    pub(super) fn new() -> Self {
        Self {
            epoch: std::time::Instant::now(),
            #[cfg(test)]
            manual: None,
            #[cfg(test)]
            failed: false,
        }
    }

    pub(super) fn now(&self) -> Option<u64> {
        #[cfg(test)]
        if self.failed {
            return None;
        }
        #[cfg(test)]
        if let Some(now) = self.manual {
            return Some(now);
        }
        u64::try_from(self.epoch.elapsed().as_nanos()).ok()
    }

    #[cfg(test)]
    pub(super) fn set_for_test(&mut self, now: u64) {
        assert!(self.manual.is_none_or(|previous| now >= previous));
        self.manual = Some(now);
    }

    #[cfg(test)]
    pub(super) fn fail_for_test(&mut self) {
        self.failed = true;
    }
}
