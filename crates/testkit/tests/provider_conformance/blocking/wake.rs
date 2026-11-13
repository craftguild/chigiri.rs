//! Event-coalescing fixture for repeated wake-driven polling.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_rate::{RateLimitDecision, RateLimitProviderError, RateLimitRequest, RateLimiter};

use super::Gate;

pub(super) struct SecondPollBlocking(pub(super) Arc<Gate>);

impl Drop for SecondPollBlocking {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl RateLimiter for SecondPollBlocking {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(SecondPollFuture {
            gate: Arc::clone(&self.0),
            first: true,
        })
    }
}

struct SecondPollFuture {
    gate: Arc<Gate>,
    first: bool,
}

impl Future for SecondPollFuture {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.first {
            self.first = false;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.gate.block();
        Poll::Pending
    }
}

pub(super) struct DelayedSecondPollBlocking {
    gate: Arc<Gate>,
    wake_sender: mpsc::SyncSender<Waker>,
}

impl DelayedSecondPollBlocking {
    pub(super) fn new(gate: Arc<Gate>, wake_done: Arc<Gate>) -> Self {
        let (wake_sender, wake_receiver) = mpsc::sync_channel::<Waker>(1);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(0);
        // Thread startup is fixture setup, outside the provider's 20ms budget.
        std::thread::spawn(move || {
            ready_sender.send(()).unwrap();
            if let Ok(waker) = wake_receiver.recv() {
                waker.wake();
            }
            wake_done.finish();
        });
        ready_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("owned wake thread must be ready before checking the provider");
        Self { gate, wake_sender }
    }
}

impl Drop for DelayedSecondPollBlocking {
    fn drop(&mut self) {
        self.gate.finish();
    }
}

impl RateLimiter for DelayedSecondPollBlocking {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(DelayedSecondPollFuture {
            gate: Arc::clone(&self.gate),
            wake_sender: self.wake_sender.clone(),
            first: true,
        })
    }
}

struct DelayedSecondPollFuture {
    gate: Arc<Gate>,
    wake_sender: mpsc::SyncSender<Waker>,
    first: bool,
}

impl Future for DelayedSecondPollFuture {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.first {
            self.first = false;
            self.wake_sender.try_send(context.waker().clone()).unwrap();
            return Poll::Pending;
        }
        self.gate.block();
        Poll::Pending
    }
}

pub(super) struct MultipleWakeProvider;

impl RateLimiter for MultipleWakeProvider {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(MultipleWakeFuture(3))
    }
}

struct MultipleWakeFuture(u8);

impl Future for MultipleWakeFuture {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 > 0 {
            self.0 -= 1;
            context.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

pub(super) struct ExcessiveWakeProvider;

impl RateLimiter for ExcessiveWakeProvider {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(MultipleWakeFuture(65))
    }
}

pub(super) struct SpamSecondPollBlocking {
    pub(super) gate: Arc<Gate>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) done: Arc<Gate>,
}

impl Drop for SpamSecondPollBlocking {
    fn drop(&mut self) {
        self.gate.finish();
    }
}

impl RateLimiter for SpamSecondPollBlocking {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(SpamSecondPollFuture {
            gate: Arc::clone(&self.gate),
            stop: Arc::clone(&self.stop),
            done: Arc::clone(&self.done),
            first: true,
        })
    }
}

struct SpamSecondPollFuture {
    gate: Arc<Gate>,
    stop: Arc<AtomicBool>,
    done: Arc<Gate>,
    first: bool,
}

impl Future for SpamSecondPollFuture {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.first {
            self.first = false;
            let waker = context.waker().clone();
            let stop = Arc::clone(&self.stop);
            let done = Arc::clone(&self.done);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    waker.wake_by_ref();
                    std::thread::yield_now();
                }
                done.finish();
            });
            return Poll::Pending;
        }
        self.gate.block();
        Poll::Pending
    }
}

pub(super) struct PendingProvider;

impl RateLimiter for PendingProvider {
    fn check_and_consume<'a>(
        &'a self,
        _: RateLimitRequest<'a>,
    ) -> chigiri_rate::RateLimitFuture<'a> {
        Box::pin(std::future::pending())
    }
}
