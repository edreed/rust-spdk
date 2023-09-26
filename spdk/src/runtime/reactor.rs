use std::{
    ffi::CString,
    iter::Map,
    os::raw::c_void,
    ptr::null_mut,
};

use futures::{
    channel::oneshot::{
        self,

        Sender,
    },
    Future,
};

use spdk_sys::{
    Errno,
    spdk_cpuset,

    spdk_event_allocate,
    spdk_event_call,
    spdk_set_thread,
    spdk_thread_bind,
    spdk_thread_create,
    spdk_thread_exit,
};

use crate::{
    task::{
        JoinHandle,
        RcTask, ReactorTask,
    },
    errors::ENOMEM,
};

use super::{
    CpuCore,
    CpuCores,
    CpuSet,

    cpu_cores,
};

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
    pub(crate) fn init(self) -> Option<JoinHandle<Sender<()>>> {
        assert!(CpuCore::current() == CpuCore::main());

        if self.is_current() {
            return None;
        }

        // Spawn an asynchronous task to initialize the reactor.
        let join_handle = self.spawn(async move {
            unsafe {
                // Create a new SPDK thread for this reactor and bind it to the
                // reactor's core.
                let name = CString::new(format!("reactor_thread_{}", self.core().id())).unwrap();
                let cpu_mask: CpuSet = self.core().into();

                let sthread = spdk_thread_create(
                    name.as_ptr(),
                    cpu_mask.as_ptr() as *mut spdk_cpuset);

                if sthread.is_null() {
                    panic!("failed to create reactor thread on core {}", self.core().id());
                }

                spdk_thread_bind(sthread, true);

                // Spawn an asynchronous task to wait for the reactor to exit.
                let (exit_sx, exit_rx) = oneshot::channel::<()>();

                self.spawn(async move {
                    let _ = exit_rx.await;

                    spdk_set_thread(sthread);
                    spdk_thread_exit(sthread);
                });

                // Return the `Sender` used to signal the reactor to exit.
                exit_sx
            }
        });

        Some(join_handle)
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
        F: FnOnce() + 'static
    {
        unsafe extern "C" fn handle_event<F: FnOnce() + 'static>(arg1: *mut c_void, _arg2: *mut c_void) {
            let f = Box::from_raw(arg1.cast::<F>());

            f();
        }

        let f = Box::new(f);

        unsafe {
            let event = spdk_event_allocate(
                self.0.into(),
                Some(handle_event::<F>),
                Box::into_raw(f).cast(),
                null_mut());
            
            if event.is_null() {
                return Err(ENOMEM);
            }

            spdk_event_call(event);
        }

        Ok(())
    }

    /// Spawns a new asynchronous task to be executed on this reactor and
    /// returns a [`JoinHandle`] to await results.
    pub fn spawn<T>(&self, fut: impl Future<Output = T> + 'static) -> JoinHandle<T>
    where
        T: 'static
    {
        let (task, join_handle) = ReactorTask::with_future(self.clone(), fut);

        RcTask::schedule(task);

        join_handle
    }
}

/// An iterator over the reactors for this runtime.
pub type Reactors = Map<CpuCores, fn(CpuCore) -> Reactor>;

/// Returns an iterator over the reactors for this runtime.
pub fn reactors() -> Reactors {
    cpu_cores().map(Reactor)
}
