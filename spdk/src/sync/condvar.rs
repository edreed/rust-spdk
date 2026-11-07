use std::ops::ControlFlow;

use event_listener::{Event, listener};
use lock_api::{Mutex, RawMutex};

/// A condition variable that can be used to asynchronously wait until a certain condition is met.
#[derive(Debug)]
pub struct Condvar {
    event: Event,
}

impl Condvar {
    /// Creates a new `Condvar` instance.
    pub fn new() -> Self {
        Self {
            event: Event::new(),
        }
    }

    /// Acquires the [`Mutex`] and calls the wait function to check if the condition has been met.
    ///
    /// # Returns
    ///
    /// This function return `ControlFlow::Break(R)` if the condition has been met, and
    /// `ControlFlow::Break` otherwise.
    ///
    /// [`Mutex`]: https://docs.rs/lock_api/latest/lock_api/struct.Mutex.html
    fn check_condition<F, M, R, T>(&self, mutex: &Mutex<M, T>, wait_fn: &mut F) -> ControlFlow<R>
    where
        M: RawMutex,
        F: FnMut(&mut T) -> ControlFlow<R>,
    {
        let mut guard = mutex.lock();
        (*wait_fn)(&mut *guard)
    }

    /// Waits asynchronously for the condition defined in the wait function to be met.
    ///
    /// The specified mutex is held while checking the condition and released while waiting. This
    /// can be any mutual exclusion primitive implemented by [`Mutex`] including [`SpinLock`].
    ///
    /// The wait function returns `ControlFlow::Break(R)` when the condition has been met, and
    /// `ControlFlow::Continue(())` to continue waiting.
    ///
    /// [`Mutex`]: https://docs.rs/lock_api/latest/lock_api/struct.Mutex.html
    /// [`SpinLock`]: super::SpinLock
    pub async fn wait<F, M, R, T>(&self, mutex: &Mutex<M, T>, mut wait_fn: F) -> R
    where
        M: RawMutex,
        F: FnMut(&mut T) -> ControlFlow<R>,
    {
        loop {
            if let ControlFlow::Break(ret) = self.check_condition(mutex, &mut wait_fn) {
                return ret;
            }

            listener!(self.event => listener);

            listener.await;
        }
    }

    /// Notifies one waiter to check the condition.
    pub fn notify_one(&self) {
        self.event.notify(1);
    }

    /// Notifies all waiters to check the condition.
    pub fn notify_all(&self) {
        self.event.notify(usize::MAX);
    }
}

impl Default for Condvar {
    fn default() -> Self {
        Self::new()
    }
}
