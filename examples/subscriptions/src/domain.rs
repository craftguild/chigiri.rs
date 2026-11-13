//! Application data and a fixed-memory owned source, independent of GraphQL.

use std::error::Error;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use futures_core::Stream;
use tokio::time::{Interval, MissedTickBehavior};

#[derive(Clone)]
pub struct Reading {
    pub number: i32,
    pub message: Arc<str>,
}

#[derive(Clone)]
pub struct State {
    message: Arc<str>,
    stats: Arc<Stats>,
}

#[derive(Default)]
struct Stats {
    started: AtomicUsize,
    dropped: AtomicUsize,
    naturally_finished: AtomicUsize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            message: Arc::from("An owned source shares this immutable domain backing."),
            stats: Arc::default(),
        }
    }
}

impl State {
    pub fn current(&self) -> Reading {
        Reading {
            number: 0,
            message: Arc::clone(&self.message),
        }
    }

    pub fn source_counts(&self) -> (usize, usize, usize) {
        (
            self.stats.started.load(Ordering::SeqCst),
            self.stats.dropped.load(Ordering::SeqCst),
            self.stats.naturally_finished.load(Ordering::SeqCst),
        )
    }

    pub(crate) fn source(&self) -> Readings {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        // Missed application clock ticks are not published domain events and
        // do not change the SDK's Backpressure or DropOldest delivery policy.
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        self.stats.started.fetch_add(1, Ordering::SeqCst);
        Readings {
            interval,
            message: Arc::clone(&self.message),
            stats: Arc::clone(&self.stats),
            next: 1,
        }
    }
}

pub(crate) struct Readings {
    interval: Interval,
    message: Arc<str>,
    stats: Arc<Stats>,
    next: i32,
}

impl Stream for Readings {
    type Item = Result<Reading, AppError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.next > 8 {
            if this.next == 9 {
                this.stats.naturally_finished.fetch_add(1, Ordering::SeqCst);
                this.next += 1;
            }
            return Poll::Ready(None);
        }
        ready!(this.interval.poll_tick(context));
        let reading = Reading {
            number: this.next,
            message: Arc::clone(&this.message),
        };
        this.next += 1;
        Poll::Ready(Some(Ok(reading)))
    }
}

impl Drop for Readings {
    fn drop(&mut self) {
        self.stats.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub struct AppError;

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("The reading source is unavailable")
    }
}

impl Error for AppError {}
