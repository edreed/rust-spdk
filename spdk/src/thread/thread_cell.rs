use std::{cell::UnsafeCell, fmt::Debug};

use crate::thread::{AsRawThread, Thread};

/// A container storing a value that is accessible only from the associated thread.
pub struct ThreadCell<T, U>
where
    T: AsRawThread,
{
    thread: Thread<T>,
    value: UnsafeCell<U>,
}

unsafe impl<T, U> Send for ThreadCell<T, U> where T: AsRawThread {}

unsafe impl<T, U> Sync for ThreadCell<T, U> where T: AsRawThread {}

impl<T, U> ThreadCell<T, U>
where
    T: AsRawThread,
{
    /// Creates a new [`ThreadCell`] associated with the specified thread and containing the value.
    pub fn new(thread: Thread<T>, value: U) -> Self {
        Self {
            thread,
            value: UnsafeCell::new(value),
        }
    }

    /// Executes the provided closure with a mutable reference to the contained value.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the closure is only called from the associated thread. If this
    /// is not the case, undefined behavior may occur.
    pub unsafe fn with_unchecked<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut U) -> R,
        R: 'static,
    {
        f(unsafe { &mut *self.value.get() })
    }

    /// Attempts to execute the provided closure with a mutable reference to the contained value.
    ///
    /// If called from the associated thread, the closure is executed immediately and its result is
    /// returned wrapped in `Some`. Otherwise, `None` is returned.
    pub fn try_with<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut U) -> R,
        R: 'static,
    {
        if self.thread.is_current() {
            return Some(f(unsafe { &mut *self.value.get() }));
        }

        None
    }

    /// Executes the provided closure with a mutable reference to the contained value.
    ///
    /// If called from the associated thread, the closure is executed immediately. Otherwise, it is
    /// spawned on the associated thread and awaited.
    ///
    /// The indirection of `fut_gen` instead of receiving an `FnOnce` directly allows for functions
    /// that may not be `Send` once invoked.
    pub async fn with<'a, G, F, R>(&self, fut_gen: G) -> R
    where
        G: FnOnce() -> F + Send + 'a,
        F: FnOnce(&mut U) -> R + 'a,
        R: Send + 'static,
    {
        if self.thread.is_current() {
            return fut_gen()(unsafe { &mut *self.value.get() });
        }

        self.thread
            .spawn(move || async move { fut_gen()(unsafe { &mut *self.value.get() }) })
            .await
    }
}

impl<T, U> Default for ThreadCell<T, U>
where
    T: AsRawThread + Default,
    U: Default,
{
    fn default() -> Self {
        Self {
            thread: Thread::default(),
            value: UnsafeCell::new(U::default()),
        }
    }
}

impl<T, U> Debug for ThreadCell<T, U>
where
    T: AsRawThread,
    U: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadCell")
            .field("thread", &self.thread)
            .field("value", unsafe { &*self.value.get() })
            .finish()
    }
}
