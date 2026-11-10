use std::ops::ControlFlow;

use event_listener::{Event, listener};
use lock_api::{Mutex, RawMutex, RawRwLock, RwLock};

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

    /// Waits asynchronously for the condition protected by the specified [`Mutex`] and defined in
    /// the wait function to be met.
    ///
    /// The specified mutex is held while checking the condition and released while waiting. This
    /// can be any mutual exclusion primitive implemented by [`Mutex`] including [`SpinLock`].
    ///
    /// The wait function returns `ControlFlow::Break(R)` when the condition has been met, and
    /// `ControlFlow::Continue(())` to continue waiting.
    ///
    /// [`Mutex`]: https://docs.rs/lock_api/latest/lock_api/struct.Mutex.html
    /// [`SpinLock`]: super::SpinLock
    pub async fn wait_on_mutex<F, M, R, T>(&self, mutex: &Mutex<M, T>, mut wait_fn: F) -> R
    where
        M: RawMutex,
        F: FnMut(&mut T) -> ControlFlow<R>,
    {
        loop {
            // Begin listening before checking the condition so that we don't miss a notification
            // that arrives **after** the lock is released but before we await the listener.
            listener!(self.event => listener);

            let flow = (wait_fn)(&mut *mutex.lock());

            if let ControlFlow::Break(ret) = flow {
                return ret;
            }

            listener.await;
        }
    }

    /// Waits asynchronously for the condition protected by the specified [`RwLock`] and defined in
    /// the wait function to be met.
    ///
    /// The specified lock is held in exclusive mode while checking the condition and released while
    /// waiting. This can be any synchronization primitive implemented by [`RwLock`].
    ///
    /// The wait function returns `ControlFlow::Break(R)` when the condition has been met, and
    /// `ControlFlow::Continue(())` to continue waiting.
    ///
    /// [`RwLock`]: https://docs.rs/lock_api/latest/lock_api/struct.RwLock.html
    pub async fn wait_on_write<F, L, R, T>(&self, mutex: &RwLock<L, T>, mut wait_fn: F) -> R
    where
        L: RawRwLock,
        F: FnMut(&mut T) -> ControlFlow<R>,
    {
        loop {
            // Begin listening before checking the condition so that we don't miss a notification
            // that arrives **after** the lock is released but before we await the listener.
            listener!(self.event => listener);

            let flow = (wait_fn)(&mut *mutex.write());

            if let ControlFlow::Break(ret) = flow {
                return ret;
            }

            listener.await;
        }
    }

    /// Waits asynchronously for the condition protected by the specified [`RwLock`] and defined in
    /// the wait function to be met.
    ///
    /// The specified lock is held in shared mode while checking the condition and released while
    /// waiting. This can be any synchronization primitive implemented by [`RwLock`].
    ///
    /// The wait function returns `ControlFlow::Break(R)` when the condition has been met, and
    /// `ControlFlow::Continue(())` to continue waiting.
    ///
    /// [`RwLock`]: https://docs.rs/lock_api/latest/lock_api/struct.RwLock.html
    pub async fn wait_on_read<F, L, R, T>(&self, mutex: &RwLock<L, T>, wait_fn: F) -> R
    where
        L: RawRwLock,
        F: Fn(&T) -> ControlFlow<R>,
    {
        loop {
            // Begin listening before checking the condition so that we don't miss a notification
            // that arrives **after** the lock is released but before we await the listener.
            listener!(self.event => listener);

            let flow = (wait_fn)(&*mutex.read());

            if let ControlFlow::Break(ret) = flow {
                return ret;
            }

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
