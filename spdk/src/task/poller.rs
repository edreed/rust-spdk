use std::{
    ffi::c_void,
    future::Future,
    mem::{transmute, MaybeUninit},
    pin::Pin,
    ptr::null_mut,
    task::{Context, Poll},
    time::Duration,
};

use spdk_sys::{
    spdk_poller, spdk_poller_pause, spdk_poller_register, spdk_poller_resume,
    spdk_poller_unregister, SPDK_POLLER_BUSY, SPDK_POLLER_IDLE,
};

/// A trait for types that can be polled.
pub trait Polled {
    /// Polls the type.
    ///
    /// Returns `true` if work was done on this invocation, `false` otherwise.
    fn poll(self: Pin<&mut Self>) -> bool;
}

/// A poller that can be registered with the SPDK event framework to poll a type implemented the
/// [`Polled`] trait.
struct PollerInner<T>
where
    T: Polled,
{
    poller: *mut spdk_poller,
    polled: T,
}

impl<T> Polled for MaybeUninit<T>
where
    T: Polled,
{
    fn poll(self: Pin<&mut Self>) -> bool {
        unreachable!("poll called on uninitialized data")
    }
}

pub struct Poller<T>(Box<PollerInner<T>>)
where
    T: Polled;

impl<T> Poller<T>
where
    T: Polled,
{
    /// Creates a new poller that will repeatedly poll `polled` as fast as possible on the current
    /// SPDK thread.
    #[inline]
    pub fn new(polled: T) -> Self {
        Self::with_period(Duration::ZERO, polled)
    }

    /// Creates a new poller that will periodically poll `polled` with the specified period on the
    /// current SPDK thread.
    ///
    /// # Notes
    ///
    /// The granularity of `period` is microseconds. If the period is zero or less than 1μs, the
    /// `polled` will be polled as fast as possible.
    ///
    /// # Panics
    ///
    /// This function panics if the duration cannot be represented as a unsigned 64-bit integer.
    pub fn with_period(period: Duration, polled: T) -> Self {
        let mut inner = Box::new(PollerInner {
            poller: null_mut(),
            polled,
        });

        inner.poller = unsafe {
            spdk_poller_register(
                Some(Self::poll),
                Box::as_mut(&mut inner) as *mut _ as *mut c_void,
                period
                    .as_micros()
                    .try_into()
                    .expect("period in range 0..2^64 - 1"),
            )
        };

        assert!(!inner.poller.is_null());

        Self(inner)
    }

    /// Creates a new poller initializing a polled object, `T`, in place. The polled object will be
    /// polled as fast as possible on the current SPDK thread.
    ///
    /// `T` is pinned in memory making it safe to pass a reference or pointer to itself to other
    /// functions.
    pub fn new_in_place<F>(init_fn: F) -> Self
    where
        F: FnOnce(Pin<&mut MaybeUninit<T>>),
    {
        Self::with_period_in_place(Duration::ZERO, init_fn)
    }

    /// Creates a new poller initializing a polled object, `T`, in place. The polled object will be
    /// polled with the specified period on the current SPDK thread.
    ///
    /// `T` is pinned in memory making it safe to pass a reference or pointer to itself to other
    /// functions.
    ///
    /// # Notes
    ///
    /// The granularity of `period` is microseconds. If the period is zero or less than 1μs, the
    /// `polled` will be polled as fast as possible.
    ///
    /// # Panics
    ///
    /// This function panics if the duration cannot be represented as a unsigned 64-bit integer.
    pub fn with_period_in_place<F>(period: Duration, init_fn: F) -> Self
    where
        F: FnOnce(Pin<&mut MaybeUninit<T>>),
    {
        let mut inner = Box::new(PollerInner {
            poller: null_mut(),
            polled: MaybeUninit::<T>::zeroed(),
        });

        init_fn(unsafe { Pin::new_unchecked(&mut inner.polled) });

        // SAFETY: The polled object was just initialized by the initialization function.
        let mut inner: Box<PollerInner<T>> = unsafe { transmute(inner) };

        inner.poller = unsafe {
            spdk_poller_register(
                Some(Self::poll),
                Box::as_mut(&mut inner) as *mut _ as *mut c_void,
                period
                    .as_micros()
                    .try_into()
                    .expect("period in range 0..2^64 - 1"),
            )
        };

        assert!(!inner.poller.is_null());

        Self(inner)
    }

    /// Returns a reference to the polled type.
    #[inline]
    pub fn polled(&self) -> &T {
        &self.0.polled
    }

    /// Returns a mutable reference to the polled type.
    #[inline]
    pub fn polled_mut(&mut self) -> &mut T {
        &mut self.0.polled
    }

    /// A callback function invoked to poll the `polled` type.
    unsafe extern "C" fn poll(arg: *mut c_void) -> i32 {
        let this = unsafe { &mut *(arg as *mut PollerInner<T>) };

        // SAFETY: The poller inner state is a heap allocation that is guaranteed not to move for
        // its lifetime. It is safe to pin its `polled` field here.
        let polled = unsafe { Pin::new_unchecked(&mut this.polled) };

        if polled.poll() {
            return SPDK_POLLER_BUSY as i32;
        }

        SPDK_POLLER_IDLE as i32
    }

    /// Pauses the poller. The polled object with not be polled until the poller is resumed by a
    /// call to the [`resume`] method.
    ///
    /// [`resume`]: method@Poller::resume
    #[inline]
    pub fn pause(&self) {
        unsafe { spdk_poller_pause(self.0.poller) }
    }

    /// Resumes a poller paused by the [`pause`] method.
    ///
    /// [`pause`]: method@Poller::pause
    #[inline]
    pub fn resume(&self) {
        unsafe { spdk_poller_resume(self.0.poller) }
    }
}

impl<T> Drop for Poller<T>
where
    T: Polled,
{
    fn drop(&mut self) {
        unsafe { spdk_poller_unregister(&mut self.0.poller) }
    }
}

impl<T, U> Future for Poller<T>
where
    T: Polled + Future<Output = U>,
{
    type Output = U;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: `polled_mut` returns a mutable reference to a field of `self` and is therefore
        // safe to pin.
        Future::poll(unsafe { self.map_unchecked_mut(|p| p.polled_mut()) }, cx)
    }
}

/// A Newtype that enables a function to be called by the poller.
pub struct PolledFn<T>(T)
where
    T: FnMut() -> bool + Unpin;

impl<T> Polled for PolledFn<T>
where
    T: FnMut() -> bool + Unpin,
{
    #[inline]
    fn poll(self: Pin<&mut Self>) -> bool {
        (self.get_mut().0)()
    }
}

/// Creates a new poller that will repeatedly poll `fun` as fast as possible on the current SPDK
/// thread.
pub fn polled_fn<T>(fun: T) -> Poller<PolledFn<T>>
where
    T: FnMut() -> bool + Unpin,
{
    Poller::new(PolledFn(fun))
}

/// Creates a new poller that will periodically poll `fun` with the specified period on the current
/// SPDK thread.
///
/// # Notes
///
/// The granularity of `period` is microseconds. If the period is zero or less than 1μs, the
/// `polled` will be polled as fast as possible.
///
/// # Panics
///
/// This function panics if the duration cannot be represented as a unsigned 64-bit integer.
pub fn polled_fn_with_period<T>(period: Duration, fun: T) -> Poller<PolledFn<T>>
where
    T: FnMut() -> bool + Unpin,
{
    Poller::with_period(period, PolledFn(fun))
}
