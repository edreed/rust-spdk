use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{channel::oneshot, Future};

/// A handle that awaits the result of a task.
///
/// Dropping a [`JoinHandle`] will detach the task leaving no way to join on
/// it or obtain its result.
///
/// A [`JoinHandle`] is created when task is spawned.
pub struct JoinHandle<T> {
    rx: oneshot::Receiver<T>,
}

impl<T> JoinHandle<T> {
    /// Create a new [`JoinHandle`] from a [`oneshot::Receiver`].
    pub(crate) fn new(rx: oneshot::Receiver<T>) -> Self {
        Self { rx }
    }

    /// Returns a pinned reference to the [`oneshot::Receiver`] used to receive
    /// the result of the task.
    pub(crate) fn rx_pin_mut(self: Pin<&mut Self>) -> Pin<&mut oneshot::Receiver<T>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.rx) }
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pinned_rx = self.rx_pin_mut();

        match pinned_rx.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(r)) => Poll::Ready(r),
            Poll::Ready(Err(_)) => panic!("sender dropped"),
        }
    }
}
