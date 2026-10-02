//! An abstraction of a lightweight, stackless thread of execution.
//!
//! An SPDK thread does not correspond 1:1 with a posix thread. Instead, a lower-level framework
//! like the SPDK Event Framework polls each SPDK thread for work. This allows the SPDK Event
//! Framework to multiplex many SPDK threads on a smaller number of posix threads.
//!
//! There are two mechanisms for scheduling work on an SPDK thread: messages and pollers. A message
//! consists of a function and single context parameter. A poller is a function that is called
//! periodically.
//!
//! See [Message Passing and Concurrency] for more details on the SPDK threading model.
//!
//! [Message Passing and Concurrency]: https://spdk.io/doc/concurrency.html
mod app;
mod borrowed;
mod owned;
#[cfg(feature = "bdev")]
mod owned_by;

use std::{
    ffi::{CStr, c_void},
    fmt::{self, Debug, Formatter},
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use futures::task::noop_waker_ref;
use spdk_sys::{
    spdk_cpuset_copy, spdk_thread, spdk_thread_bind, spdk_thread_get_cpumask, spdk_thread_get_id,
    spdk_thread_get_name, spdk_thread_is_bound, spdk_thread_poll, spdk_thread_send_msg,
};

use crate::{
    Result,
    runtime::CpuSet,
    task::{ArcTask, Executor, JoinHandle, LocalTask, RcTask, RemoteTask},
    to_result,
};

pub use app::App;
pub use borrowed::Borrowed;
pub use owned::Owned;
#[cfg(feature = "bdev")]
pub use owned_by::OwnedBy;

/// A trait for SPDK thread types that provides access to the underlying raw `spdk_thread` pointer.
pub trait AsRawThread {
    /// Returns a raw pointer to the underlying `spdk_thread` structure.
    fn as_raw_thread(&self) -> *mut spdk_thread;
}

/// An abstraction of a lightweight, stackless thread of execution.
///
/// `Thread` wraps an `spdk_thread` pointer and can be in one of two ownership states: owned or
/// borrowed.
///
/// TODO: Refine documentation for new type-based ownership semantics.
///
/// An owned thread owns the underlying `spdk_thread` pointer and will mark it for exit when
/// dropped. Any further processing requests on this thread will fail.
///
/// A borrowed thread borrows the underlying `spdk_thread` pointer. Dropping a borrowed thread has
/// no effect on the underlying `spdk_thread` pointer.
pub struct Thread<T>(T)
where
    T: AsRawThread;

unsafe impl<T> Send for Thread<T> where T: AsRawThread + Send {}
unsafe impl<T> Sync for Thread<T> where T: AsRawThread + Sync {}

impl<T> Thread<T>
where
    T: AsRawThread,
{
    /// Returns a pointer to the underlying `spdk_thread` structure.
    fn as_ptr(&self) -> *mut spdk_thread {
        self.0.as_raw_thread()
    }

    /// Borrows the thread object.
    ///
    /// # Notes
    ///
    /// Dropping the borrow has no effect on the underlying thread object.
    pub fn borrow(&self) -> Thread<Borrowed> {
        Thread(unsafe { Borrowed::new_unchecked(self.as_ptr()) })
    }

    /// Returns the name of this thread.
    pub fn name(&self) -> &CStr {
        unsafe {
            let name = spdk_thread_get_name(self.as_ptr());

            CStr::from_ptr(name)
        }
    }

    /// Returns the unique identifier for this thread.
    pub fn id(&self) -> u64 {
        unsafe { spdk_thread_get_id(self.as_ptr()) }
    }

    /// Bind or unbind the thread to its current CPU core.
    pub fn bind(&mut self, bind: bool) {
        unsafe { spdk_thread_bind(self.as_ptr(), bind) }
    }

    /// Returns whether this thread is the current thread.
    pub fn is_current(&self) -> bool {
        if let Some(t) = Thread::<Borrowed>::try_current() {
            return self.as_ptr() == t.as_ptr();
        }

        false
    }

    /// Returns whether the thread is bound to its current CPU core.
    pub fn is_bound(&self) -> bool {
        unsafe { spdk_thread_is_bound(self.as_ptr() as *mut _) }
    }

    /// Returns the CPU set for this thread.
    pub fn cpuset(&self) -> CpuSet {
        unsafe {
            let mut cpuset = MaybeUninit::uninit();

            spdk_cpuset_copy(
                cpuset.as_mut_ptr(),
                spdk_thread_get_cpumask(self.as_ptr() as *mut _),
            );

            cpuset.assume_init().into()
        }
    }

    /// Invokes a function sent via [`Thread::send_msg()`] on the current thread.
    ///
    /// [`Thread::send_msg()`]: method@Thread::send_msg
    unsafe extern "C" fn handle_msg<F>(ctx: *mut c_void)
    where
        F: FnOnce(),
    {
        let msg_fn = unsafe { Box::from_raw(ctx as *mut F) };

        (*msg_fn)();
    }

    /// Sends a message function to be executed on this thread.
    ///
    /// The message is sent asynchronously. This function may return before the message function is
    /// called.
    ///
    /// # Return
    ///
    /// This function returns `Ok(())` if the message function was successfully queued.
    ///
    /// This function return [`ENOMEM`] if the message could not be allocated and [`EIO`] if the
    /// message could not be sent to the destination thread.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use spdk::thread::Thread;
    ///
    /// let t = Thread::current();
    ///
    /// assert!(t.send_msg(|| println!("Hello, World!")).is_ok());
    /// ```
    ///
    /// [`EIO`]: crate::errors::EIO
    /// [`ENOMEM`]: crate::errors::ENOMEM
    pub fn send_msg<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(),
    {
        let ctx = Box::into_raw(Box::new(f)).cast();

        unsafe {
            to_result!(spdk_thread_send_msg(
                self.as_ptr(),
                Some(Self::handle_msg::<F>),
                ctx
            ))
        }
    }

    /// Perform one iteration worth of processing on the thread.
    ///
    /// # Returns
    ///
    /// This method returns `true` if work was done and `false` otherwise.
    pub(crate) fn poll(&self) -> bool {
        unsafe { spdk_thread_poll(self.as_ptr() as *mut _, 0, 0) != 0 }
    }

    /// Spawns a new asynchronous task to be executed on this thread and returns a [`JoinHandle`] to
    /// await results.
    ///
    /// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures
    /// that may not be `Send` once started.
    #[must_use = " the returned JoinHandle must be awaited"]
    pub fn spawn<'a, G, F, R>(&self, fut_gen: G) -> JoinHandle<'a, F, R>
    where
        G: FnOnce() -> F + Send + 'a,
        F: Future<Output = R> + 'a,
        R: Send + 'static,
    {
        let task = RemoteTask::new(self.borrow(), fut_gen());

        ArcTask::schedule_by_ref(&task);

        JoinHandle::from_remote_task(task)
    }

    /// Spawns a new asynchronous task to be executed on this thread that will run to completion
    /// independently of the current thread.
    ///
    /// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures
    /// that may not be `Send` once started.
    pub fn spawn_detached<G, F, R>(&self, fut_gen: G)
    where
        G: FnOnce() -> F + Send + 'static,
        F: Future<Output = R> + 'static,
        R: Send + 'static,
    {
        let task = RemoteTask::new(self.borrow(), fut_gen());

        ArcTask::schedule(task);
    }
}

impl Thread<App> {
    /// Returns the application thread object.
    ///
    /// The application thread is the thread that initialized the SPDK Application Framework.
    pub fn application() -> Self {
        Self(App)
    }
}

impl Thread<Borrowed> {
    /// Tries to return the current thread object.
    ///
    /// # Return
    ///
    /// If the current system thread is an SPDK thread, this function returns `Some(Self)`.
    /// Otherwise, this function returns `None`.
    pub fn try_current() -> Option<Self> {
        Borrowed::try_current().map(Self)
    }

    /// Returns the current thread object.
    ///
    /// # Panics
    ///
    /// This function panics if the current system thread is not an SPDK thread.
    pub fn current() -> Self {
        Self::try_current().expect("must be called on SPDK thread")
    }
}

impl Thread<Owned> {
    /// Creates a new owned thread.
    ///
    /// # Notes
    ///
    /// The thread object returned is owned by the caller. When dropped, the thread will be marked
    /// for exit causing any further processing requests on this thread to fail.
    pub fn new(name: &CStr, cpuset: &CpuSet) -> Result<Self> {
        Ok(Self(Owned::new(name, cpuset)?))
    }
}

#[cfg(feature = "bdev")]
impl<'a, T> Thread<OwnedBy<'a, T>> {
    pub(crate) unsafe fn with_owner(owner: &'a T, thread: *mut spdk_thread) -> Self {
        Self(unsafe { OwnedBy::with_owner(owner, thread) })
    }
}

impl<T> Executor for Thread<T>
where
    T: AsRawThread,
{
    fn is_current(&self) -> bool {
        self.is_current()
    }

    fn schedule<F>(&self, f: F)
    where
        F: FnOnce(),
    {
        self.send_msg(f).expect("thread message sent");
    }
}

impl<T> Debug for Thread<T>
where
    T: AsRawThread,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Thread(\"{}\")", self.name().to_string_lossy())
    }
}

/// Creates a new [`Thread`] to execute an asynchronous task and returns a [`JoinHandle`] to
/// await results.
///
/// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures
/// that may not be `Send` once started.
#[must_use = " the returned JoinHandle must be awaited"]
pub fn spawn<'a, G, F, R>(name: &CStr, cpuset: &CpuSet, fut_gen: G) -> Result<JoinHandle<'a, F, R>>
where
    G: FnOnce() -> F + Send + 'a,
    F: Future<Output = R> + 'a,
    R: Send + 'static,
{
    let thread = Thread::new(name, cpuset)?;
    let fut = fut_gen();
    let task = RemoteTask::new(thread.borrow(), async move {
        let res = fut.await;
        drop(thread);
        res
    });

    ArcTask::schedule_by_ref(&task);

    Ok(JoinHandle::from_remote_task(task))
}

/// Creates a new [`Thread`] to execute an asynchronous task independently of the current task.
///
/// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures
/// that may not be `Send` once started.
#[must_use = " the returned JoinHandle must be awaited"]
pub fn spawn_detached<G, F, R>(name: &CStr, cpuset: &CpuSet, fut_gen: G) -> Result<()>
where
    G: FnOnce() -> F + Send + 'static,
    F: Future<Output = R> + 'static,
    R: Send + 'static,
{
    let thread = Thread::new(name, cpuset)?;
    let fut = fut_gen();
    let task = RemoteTask::new(thread.borrow(), async move {
        let res = fut.await;
        drop(thread);
        res
    });

    ArcTask::schedule(task);

    Ok(())
}

/// Spawns a new asynchronous task to be executed on the current SPDK thread and returns a
/// [`JoinHandle`] to await results.
#[must_use = " the returned JoinHandle must be awaited"]
pub fn spawn_local<'a, F, R>(fut: F) -> JoinHandle<'a, F, R>
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    let task = LocalTask::new(Thread::current(), fut);

    RcTask::schedule_by_ref(&task);

    JoinHandle::from_local_task(task)
}

/// Spawns a new asynchronous task to be executed on the current SPDK thread that runs to completion
/// independently of the current task.
pub fn spawn_local_detached<F, R>(fut: F)
where
    F: Future<Output = R> + 'static,
    R: 'static,
{
    let task = LocalTask::new(Thread::current(), fut);

    RcTask::schedule(task);
}

/// Runs the provided future on the current SPDK thread until completion.
///
/// # Notes
///
/// This function blocks the current reactor until the future completes on the current SPDK thread.
/// Although the given future may spawn concurrent tasks on this thread, tasks on other threads
/// associated with the current reactor will not run. The given future must not depend on the result
/// of concurrent tasks associated with other threads, otherwise a deadlock will occur.
pub fn block_on<'a, F, R>(fut: F) -> R
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    let current_thread = Thread::current();
    let mut join_handle = spawn_local(fut);

    loop {
        if let Poll::Ready(res) =
            Pin::new(&mut join_handle).poll(&mut Context::from_waker(noop_waker_ref()))
        {
            return res;
        }

        current_thread.poll();
    }
}
