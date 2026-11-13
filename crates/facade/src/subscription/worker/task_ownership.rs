//! Application disposal evidence remains attached to the original managed task.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};

use super::dispose_value;
use crate::managed::EventActivity;

pub(crate) struct TaskOwnedValue<T> {
    value: Option<T>,
    activity: Option<EventActivity>,
}

impl<T> TaskOwnedValue<T> {
    pub(crate) fn new(value: T, activity: Option<EventActivity>) -> Self {
        Self {
            value: Some(value),
            activity,
        }
    }

    pub(crate) fn into_inner(mut self) -> T {
        self.value.take().expect("live task-owned value")
    }
}

impl<T> Deref for TaskOwnedValue<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().expect("live task-owned value")
    }
}
impl<T> DerefMut for TaskOwnedValue<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("live task-owned value")
    }
}

impl<T> Drop for TaskOwnedValue<T> {
    fn drop(&mut self) {
        let value = self.value.take();
        if let Some(activity) = &self.activity
            && activity.is_managed()
        {
            if !value.is_none_or(dispose_value) {
                activity.poison_task();
            }
            return;
        }
        drop(value);
    }
}

pub(crate) struct TaskOwnedFuture<F: Future> {
    future: TaskOwnedValue<Pin<Box<F>>>,
}

impl<F: Future> TaskOwnedFuture<F> {
    pub(crate) fn new(future: F, activity: Option<EventActivity>) -> Self {
        Self {
            future: TaskOwnedValue::new(Box::pin(future), activity),
        }
    }
}

impl<F: Future> Future for TaskOwnedFuture<F> {
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.future.as_mut().poll(context)
    }
}
