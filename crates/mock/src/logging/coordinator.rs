//! Drain three reserved stdout classes fairly through bounded confirmation pipelines.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;

use super::{Counters, Kind, Loss, Record, Stop, pipe::Pipe, records};

pub(super) fn run(
    executable: &Path,
    stdout: [Receiver<Record>; 3],
    notices: Receiver<Record>,
    counts: Arc<Counters>,
    stop: Arc<Mutex<Stop>>,
) -> bool {
    let mut output = Pipe::start(executable, Ok(Stdio::inherit()));
    let stderr = chigiri_host::stdio::inherited_stderr()
        .map_err(|_| std::io::Error::other("stderr unavailable"));
    let mut error = Pipe::start(executable, stderr);
    let mut pending_loss = None;
    let mut pending_notice = None;
    let mut announced = Loss::default();
    let mut report_at = Instant::now();
    let mut cursor = 0;
    let deadline = loop {
        output.pump(&counts);
        error.pump(&counts);
        let current = counts.snapshot();
        if current != announced && Instant::now() >= report_at {
            pending_loss = record(Kind::Loss, records::loss(current));
            pending_notice = record(Kind::Loss, records::notice(&loss_notice(current)));
            announced = current;
            report_at = Instant::now() + Duration::from_secs(1);
        }
        if output.idle() {
            let next = pending_loss
                .take()
                .or_else(|| next_stdout(&stdout, &mut cursor));
            if let Some(record) = next {
                output.put(record, &counts);
            }
        }
        if error.idle()
            && let Some(record) = notices.try_recv().ok().or_else(|| pending_notice.take())
        {
            error.put(record, &counts);
        }
        let mut stopping = stop.lock().expect("logging stop lock");
        if let Some(deadline) = stopping.deadline {
            let drained = stdout.iter().all(Receiver::is_empty)
                && notices.is_empty()
                && output.idle()
                && error.idle();
            if (stopping.close && drained)
                || Instant::now()
                    >= deadline
                        .checked_sub(Duration::from_millis(300))
                        .unwrap_or(deadline)
            {
                stopping.close = true;
                break deadline;
            }
        }
        drop(stopping);
        std::thread::sleep(Duration::from_millis(2));
    };
    // Count discarded records once, then make one bounded final attempt to publish the loss notice.
    output.discard(&counts);
    error.discard(&counts);
    discard_queues(&stdout, &notices, &counts);
    let loss = counts.snapshot();
    if loss != Loss::default() {
        if let Some(record) = record(Kind::Loss, records::loss(loss)) {
            output.put(record, &counts);
        }
        if let Some(record) = record(Kind::Loss, records::notice(&loss_notice(loss))) {
            error.put(record, &counts);
        }
        let until = deadline
            .checked_sub(Duration::from_millis(150))
            .unwrap_or(deadline);
        while Instant::now() < until && (!output.idle() || !error.idle()) {
            output.pump(&counts);
            error.pump(&counts);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let stdout_reaped = output.stop(deadline);
    let stderr_reaped = error.stop(deadline);
    stdout_reaped && stderr_reaped
}

pub(super) fn discard_queues(
    stdout: &[Receiver<Record>; 3],
    notices: &Receiver<Record>,
    counts: &Counters,
) {
    for (queue, capacity) in stdout
        .iter()
        .map(|queue| (queue, super::QUEUE_RECORDS))
        .chain(std::iter::once((notices, super::NOTICE_RECORDS)))
    {
        // A competing producer cannot extend the fixed drain window.
        for record in queue.try_iter().take(capacity) {
            counts.lose(record.kind);
        }
    }
}
fn record(kind: Kind, result: Result<Vec<u8>, crate::value::ValueError>) -> Option<Record> {
    result.ok().map(|bytes| Record { kind, bytes })
}
pub(super) fn next_stdout(queues: &[Receiver<Record>; 3], cursor: &mut usize) -> Option<Record> {
    for _ in 0..queues.len() {
        let current = *cursor;
        *cursor = (*cursor + 1) % queues.len();
        if let Ok(record) = queues[current].try_recv() {
            return Some(record);
        }
    }
    None
}
fn loss_notice(loss: Loss) -> String {
    format!(
        "chigiri mock log loss: hook={} access={} subscription={} notice={}",
        loss.hook, loss.access, loss.subscription, loss.notice
    )
}
