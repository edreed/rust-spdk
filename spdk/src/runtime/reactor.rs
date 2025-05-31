use std::{ffi::CString, iter::Map, os::raw::c_void, ptr::null_mut};

use futures::{
    channel::oneshot::{self, Sender},
    Future,
};

use spdk_sys::{spdk_event_allocate, spdk_event_call};

use crate::{
    errors::{Errno, ENOMEM},
    task::{JoinHandle, ReactorTask, Task},
    thread::Thread,
};

use super::{cpu_cores, CpuCore, CpuCores, CpuSet};

/// Represents an event loop running in a OS thread bound to a dedicated CPU
/// core that drives execution of asynchronous tasks.
#[derive(Clone, Copy, PartialEq)]
pub struct Reactor(CpuCore);

impl Reactor {
    /// Initializes this reactor.
    ///
    /// # Returns
    ///
    /// This function returns a [`JoinHandle<_>`] used to await a
    /// [`Sender<()>`]. The `Sender` is used to signal the reactor to exit.
    /// Since the main reactor cannot await itself, this function returns `None`
    /// if called from the main CPU core.
    ///
    /// # Panics
    ///
    /// This function panics if not called from the main CPU core.
    pub(crate) fn init(self) -> Option<Sender<()>> {
        assert!(CpuCore::current() == CpuCore::main());

        if self.is_current() {
            return None;
        }

        // Create a new SPDK thread for this reactor and bind it to the
        // reactor's core.
        let name = CString::new(format!("reactor_thread_{}", self.core().id())).unwrap();
        let cpu_mask: CpuSet = self.core().into();

        let mut owned_thread = Thread::new(&name, &cpu_mask).expect("thread created");

        owned_thread.bind(true);

        // Spawn an asynchronous task to wait for the reactor to exit. We borrow
        // the reactor thread so that we can transfer ownership to the
        // asynchronous task and exit the thread when it is dropped.
        let (exit_sx, exit_rx) = oneshot::channel::<()>();
        let borrowed_thread = owned_thread.borrow();

        borrowed_thread.spawn(move || async move {
            let _ = exit_rx.await;

            drop(owned_thread)
        });

        // Return the `Sender` used to signal the reactor to exit.
        Some(exit_sx)
    }

    /// Tries to return the current reactor.
    ///
    /// # Returns
    ///
    /// If the current system thread is running an SPDK Reactor, this function
    /// returns `Some(r)` where `r` is the current reactor. Otherwise, this
    /// function returns `None`.
    pub fn try_current() -> Option<Self> {
        match CpuCore::try_current() {
            Some(core) => Some(Self(core)),
            None => None,
        }
    }

    /// Returns the current reactor.
    ///
    /// # Panics
    ///
    /// This function panics if the current system thread is not running an SPDK
    /// Reactor.
    pub fn current() -> Self {
        Self::try_current().expect("must be called on an SPDK Reactor")
    }

    /// Returns whether this reactor is the current reactor.
    pub fn is_current(&self) -> bool {
        if let Some(r) = Self::try_current() {
            return r == *self;
        }

        false
    }

    /// Returns the main reactor for this runtime.
    pub fn main() -> Self {
        Reactor(CpuCore::main())
    }

    /// Returns the dedicated CPU core this reactor is bound to.
    pub fn core(&self) -> CpuCore {
        self.0
    }

    pub fn send_event<F>(&self, f: F) -> Result<(), Errno>
    where
        F: FnOnce() + 'static,
    {
        unsafe extern "C" fn handle_event<F: FnOnce() + 'static>(
            arg1: *mut c_void,
            _arg2: *mut c_void,
        ) {
            let f = Box::from_raw(arg1.cast::<F>());

            f();
        }

        let f = Box::new(f);

        unsafe {
            let event = spdk_event_allocate(
                self.0.into(),
                Some(handle_event::<F>),
                Box::into_raw(f).cast(),
                null_mut(),
            );

            if event.is_null() {
                return Err(ENOMEM);
            }

            spdk_event_call(event);
        }

        Ok(())
    }

    /// Spawns a new asynchronous task to be executed on this reactor and
    /// returns a [`JoinHandle`] to await results.
    ///
    /// The indirection of `fut_gen` instead of receiving a `Future` directly
    /// allows for futures that may not be `Send` once started.
    pub fn spawn<G, F, T>(&self, fut_gen: G) -> JoinHandle<T>
    where
        G: FnOnce() -> F + Send + 'static,
        F: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        let (task, join_handle) = ReactorTask::with_future(self.clone(), fut_gen());

        Task::schedule(task);

        join_handle
    }
}

/// Spawns a new asynchronous task to be executed on the current SPDK reactor and
/// returns a [`JoinHandle`] to await results.
pub fn spawn_local<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let (task, join_handle) = ReactorTask::with_future(Reactor::current(), fut);

    Task::schedule(task);

    join_handle
}

/// An iterator over the reactors for this runtime.
pub type Reactors = Map<CpuCores, fn(CpuCore) -> Reactor>;

/// Returns an iterator over the reactors for this runtime.
pub fn reactors() -> Reactors {
    cpu_cores().map(Reactor)
}
