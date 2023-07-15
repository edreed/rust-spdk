use std::{
    option::Option,
    os::raw::c_void,
    ptr::null_mut,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::{Instant, Duration},
};

use futures::future::poll_fn;
use spdk_sys::{
    spdk_poller,
    spdk_poller_register,
    spdk_poller_unregister,
    SPDK_POLLER_IDLE,
    SPDK_POLLER_BUSY,
};

/// Encapsulates the execution state of an [`Interval`].
struct IntervalState {
    ticker: u32,
    waker: Option<Waker>
}

/// A time interval returned by [`interval`].
/// 
/// [`Interval`] enables you to wait asynchronously on a sequence of instants
/// with a fixed period.
pub struct Interval {
    poller: *mut spdk_poller,
    state: Rc<IntervalState>
}

impl Interval {
    /// Completes when the next instant in the interval has been reached.
    pub async fn tick(&mut self) -> Instant {
        let tick_fut = poll_fn(move |ctx| self.poll_tick(ctx));

        tick_fut.await
    }

    /// Polls for the next instant in the interval to be reached.
    fn poll_tick(&mut self, ctx: &mut Context<'_>) -> Poll<Instant> {
        let mut inner = Rc::get_mut(&mut self.state).unwrap();

        if inner.ticker == 0 {
            inner.waker = Some(ctx.waker().clone());
            return Poll::Pending;
        }

        inner.ticker = 0;

        Poll::Ready(Instant::now())
    }
}

impl Drop for Interval {
    fn drop(&mut self) {
        unsafe { spdk_poller_unregister(&mut self.poller as *mut _) }
    }
}

unsafe impl Send for Interval {}
unsafe impl Sync for Interval {}

/// Creates a new [`Interval`] that yields with an interval of `period`.
/// 
/// The first tick completes after the period has elapsed and with the
/// specified period thereafter.
/// 
/// The ticker is driven by an SPDK polling function registered on the current
/// thread.
/// 
/// TODO: Document drift behavior.
pub fn interval(period: Duration) -> Interval {
    unsafe extern "C" fn poll(ctx: *mut c_void) -> i32 {
        let mut inner = &mut *(ctx as *mut IntervalState);

        inner.ticker += 1;
        
        if let Some(waker) = inner.waker.take() {
            waker.wake();
            return SPDK_POLLER_BUSY.try_into().unwrap()
        }

        SPDK_POLLER_IDLE.try_into().unwrap()
    }

    let period_us = period.as_micros().try_into().unwrap();
    let mut interval = Interval {
        poller: null_mut(),
        state: Rc::new(IntervalState {
            ticker: 0,
            waker: None,
        }),
    };
    let poller = unsafe {
        spdk_poller_register(
            Some(poll),
            Rc::as_ptr(&interval.state) as *mut c_void,
            period_us
        )
    };

    interval.poller = poller;

    interval
}

/// Waits until `duration` has elapsed.
pub async fn sleep(duration: Duration) {
    _ = interval(duration).tick().await;
}
