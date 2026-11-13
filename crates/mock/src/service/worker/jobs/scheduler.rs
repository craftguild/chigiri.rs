//! Execute bounded VM jobs while the same Worker owns independent source delivery.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, TryRecvError};

use super::{Job, execute_job, reject};
use crate::cancellation::{CancelOnDrop, Cancellation};
use crate::luau::{HookScript, ScriptSet};
use crate::publication::PublicationStore;

const IDLE_WAIT: Duration = Duration::from_millis(20);

pub(in crate::service::worker) fn run(
    store: Arc<Mutex<PublicationStore>>,
    scripts: ScriptSet,
    hooks: Vec<HookScript>,
    receiver: Receiver<Job>,
    shutdown: Cancellation,
    queue_open: Arc<Mutex<bool>>,
    mut consumers: super::super::consumers::ConsumerSet,
) {
    let _cancel = CancelOnDrop(shutdown.clone());
    while !shutdown.cancelled() {
        if crate::vm_gate::poisoned() {
            shutdown.cancel();
            break;
        }
        let progressed = consumers.step(&store, &shutdown);
        if progressed {
            match receiver.try_recv() {
                Ok(job) => execute_job(&store, &scripts, &hooks, job, &shutdown),
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
        } else {
            crossbeam_channel::select! {
                recv(receiver) -> job => match job {
                    Ok(job) => execute_job(&store, &scripts, &hooks, job, &shutdown),
                    Err(_) => break,
                },
                recv(consumers.wake) -> _ => {},
                default(IDLE_WAIT) => {}
            }
        }
    }

    close_admission(&queue_open, &receiver);
}

fn close_admission(queue_open: &Arc<Mutex<bool>>, receiver: &Receiver<Job>) {
    *queue_open.lock().expect("worker acceptance lock") = false;
    for job in receiver.try_iter() {
        reject(job, true);
    }
}
