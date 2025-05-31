use std::{
    ffi::c_void,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::Deref,
    ptr::{addr_of_mut, NonNull},
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
    fn poll(&mut self) -> bool;
}

/// A poller that can be registered with the SPDK event framework to poll a type
/// implemented the [`Polled`] trait.
pub struct Poller<'a, T>
where
    T: Polled,
{
    poller: NonNull<spdk_poller>,
    polled: PhantomData<&'a mut T>,
}

impl<'a, T> Poller<'a, T>
where
    T: Polled,
{
    /// Creates a new poller that will repeatedly poll `polled` as fast as
    /// possible on the current SPDK thread.
    #[inline]
    pub fn new(polled: &'a mut T) -> Self {
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
    pub fn with_period(polled: &'a mut T, period: Duration) -> Self {
        let poller = NonNull::new(unsafe {
            spdk_poller_register(
                Some(Self::poll),
                polled as *mut T as *mut _,
                period
                    .as_micros()
                    .try_into()
                    .expect("period in range 0..2^64"),
            )
        })
        .expect("poller created");

        Self {
            poller,
            polled: PhantomData,
        }
    }

    /// A callback function invoked to poll the `polled` type.
    unsafe extern "C" fn poll(arg: *mut c_void) -> i32 {
        let polled = unsafe { &mut *(arg as *mut T) };

        if polled.poll() {
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
        unsafe { spdk_poller_pause(self.poller.as_ptr()) }
    }

    /// Resumes a poller paused by the [`pause`] method.
    ///
    /// [`pause`]: method@Poller::pause
    #[inline]
    pub fn resume(&self) {
        unsafe { spdk_poller_resume(self.poller.as_ptr()) }
    }
}

impl<'a, T> Drop for Poller<'a, T>
where
    T: Polled,
{
    fn drop(&mut self) {
        unsafe { spdk_poller_unregister(&mut self.poller.as_ptr()) }
    }
}

/// A poller that polls a function.
pub struct PolledFn<'a, T: FnMut() -> bool> {
    fun: T,
    poller: Poller<'a, Self>,
}

impl<'a, T> PolledFn<'a, T>
where
    T: FnMut() -> bool,
{
    /// Creates a new poller that will repeatedly poll `fun` as fast as possible
    /// on the current SPDK thread.
    #[inline]
    pub fn new(fun: T) -> Box<Self> {
        Self::with_period(fun, Duration::ZERO)
    }

    /// Creates a new poller that will periodically poll `fun` with the
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
    pub fn with_period(fun: T, period: Duration) -> Box<Self> {
        let this = Box::new(MaybeUninit::<Self>::uninit());
        let this = Box::into_raw(this) as *mut Self;

        unsafe {
            addr_of_mut!((*this).fun).write(fun);
            addr_of_mut!((*this).poller).write(Poller::with_period(&mut *this, period));

            Box::from_raw(this)
        }
    }
}

impl<'a, T> Polled for PolledFn<'a, T>
where
    T: FnMut() -> bool,
{
    #[inline]
    fn poll(&mut self) -> bool {
        (self.fun)()
    }
}

impl<'a, T> Deref for PolledFn<'a, T>
where
    T: FnMut() -> bool,
{
    type Target = Poller<'a, Self>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.poller
    }
}

/// Creates a new poller that will repeatedly poll `fun` as fast as possible on
/// the current SPDK thread.
pub fn polled_fn<T>(fun: T) -> Box<PolledFn<'static, T>>
where
    T: FnMut() -> bool + 'static,
{
    PolledFn::new(fun)
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
pub fn polled_fn_with_period<T>(fun: T, period: Duration) -> Box<PolledFn<'static, T>>
where
    T: FnMut() -> bool + 'static,
{
    PolledFn::with_period(fun, period)
}
