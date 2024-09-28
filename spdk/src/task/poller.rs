use std::{
    ffi::c_void,
    ptr::null_mut,
    time::Duration,
};

use spdk_sys::{
    spdk_poller,

    SPDK_POLLER_BUSY,
    SPDK_POLLER_IDLE,

    spdk_poller_pause,
    spdk_poller_register,
    spdk_poller_resume,
    spdk_poller_unregister,
};

/// A trait for types that can be polled.
pub trait Polled {
    /// Polls the type.
    /// 
    /// Returns `true` if work was done on this invocation, `false` otherwise.
    fn poll(&mut self) -> bool;
}

/// A poller that can be registered with the SPDK event framework to poll a type
/// implemented the [`Polled`] trait.
struct PollerInner<T>
where
    T: Polled
{
    poller: *mut spdk_poller,
    polled: T,
}

pub struct Poller<T>
where
    T: Polled
{
    inner: Box<PollerInner<T>>,
}

impl<T> Poller<T>
where
    T: Polled
{
    /// Creates a new poller that will repeatedly poll `polled` as fast as
    /// possible on the current SPDK thread.
    #[inline]
    pub fn new(polled: T) -> Self {
        Self::with_period(polled, Duration::ZERO)
    }

    /// Creates a new poller that will periodically poll `polled` with the
    /// specified period on the current SPDK thread.
    /// 
    /// # Notes
    /// 
    /// The granularity of `period` is microseconds. If the period is zero or
    /// less than 1μs, the `polled` will be polled as fast as possible.
    /// 
    /// # Panics
    /// 
    /// This function panics if the duration cannot be represented as a unsigned
    /// 64-bit integer.
    pub fn with_period(polled: T, period: Duration) -> Self {
        let mut inner = Box::new(PollerInner {
            poller: null_mut(),
            polled,
        });

        inner.poller = unsafe {
            spdk_poller_register(
                Some(Self::poll),
                Box::as_mut(&mut inner) as *mut _ as *mut c_void,
                period.as_micros().try_into().expect("period in range 0..2^64"))
        };

        assert!(!inner.poller.is_null());

        Self {
            inner,
        }
    }

    /// Returns a reference to the polled type.
    pub fn polled(&self) -> &T {
        &self.inner.polled
    }

    /// Returns a mutable reference to the polled type.
    pub fn polled_mut(&mut self) -> &mut T {
        &mut self.inner.polled
    }

    /// A callback function invoked to poll the `polled` type.
    unsafe extern "C" fn poll(arg: *mut c_void) -> i32 {
        let this = unsafe { &mut *(arg as *mut PollerInner<T>) };

        if this.polled.poll() {
            return SPDK_POLLER_BUSY as i32;
        }

        SPDK_POLLER_IDLE as i32
    }

    /// Pauses the poller. The polled object with not be polled until the poller
    /// is resumed by a call to the [`resume`] method.
    /// 
    /// [`resume`]: method@Poller::resume
    #[inline]
    pub fn pause(&self) {
        unsafe { spdk_poller_pause(self.inner.poller) }
    }

    /// Resumes a poller paused by the [`pause`] method.
    /// 
    /// [`pause`]: method@Poller::pause
    #[inline]
    pub fn resume(&self) {
        unsafe { spdk_poller_resume(self.inner.poller) }
    }
}

impl<T> Drop for Poller<T>
where
    T: Polled
{
    fn drop(&mut self) {
        unsafe { spdk_poller_unregister(&mut self.inner.poller) }
    }
}

/// A Newtype that enables a function to be called by the poller.
pub struct PolledFn<T>(T)
where
    T: FnMut() -> bool;

impl<T> Polled for PolledFn<T>
where
    T: FnMut() -> bool
{
    #[inline]
    fn poll(&mut self) -> bool {
        (self.0)()
    }
}

/// Creates a new poller that will repeatedly poll `fun` as fast as possible on
/// the current SPDK thread.
pub fn polled_fn<T>(fun: T) -> Poller<PolledFn<T>>
where
    T: FnMut() -> bool + 'static
{
    Poller::new(PolledFn(fun))
}

/// Creates a new poller that will periodically poll `fun` with the specified
/// period on the current SPDK thread.
/// 
/// # Notes
/// 
/// The granularity of `period` is microseconds. If the period is zero or
/// less than 1μs, the `polled` will be polled as fast as possible.
/// 
/// # Panics
/// 
/// This function panics if the duration cannot be represented as a unsigned
/// 64-bit integer.
pub fn polled_fn_with_period<T>(fun: T, period: Duration) -> Poller<PolledFn<T>>
where
    T: FnMut() -> bool + 'static
{
    Poller::with_period(PolledFn(fun), period)
}
