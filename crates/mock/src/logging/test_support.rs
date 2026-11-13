//! Capture bounded built-in subscription logs without starting a writer process.

use crossbeam_channel::Receiver;

use super::{Logger, NOTICE_RECORDS, QUEUE_RECORDS, Record, bounded};

pub(crate) struct SubscriptionLogs {
    pub(crate) logger: Logger,
    records: Receiver<Record>,
    _other_classes: [Receiver<Record>; 3],
}

impl SubscriptionLogs {
    pub(crate) fn new() -> Self {
        let (hook, hooks) = bounded(QUEUE_RECORDS);
        let (access, accesses) = bounded(QUEUE_RECORDS);
        let (subscription, records) = bounded(QUEUE_RECORDS);
        let (notice, notices) = bounded(NOTICE_RECORDS);
        Self {
            logger: Logger {
                hook,
                access,
                subscription,
                notice,
                counts: Default::default(),
                stopping: Default::default(),
            },
            records,
            _other_classes: [hooks, accesses, notices],
        }
    }

    pub(crate) fn take_hooks(&self) -> Vec<serde_json::Value> {
        self._other_classes[0]
            .try_iter()
            .map(|record| serde_json::from_slice(&record.bytes).unwrap())
            .collect()
    }

    pub(crate) fn take(&self) -> Vec<serde_json::Value> {
        self.records
            .try_iter()
            .map(|record| serde_json::from_slice(&record.bytes).unwrap())
            .collect()
    }
}
