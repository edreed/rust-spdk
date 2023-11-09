//! Utilities for tracking time.
//! 
//! This module provides functions for asynchronously executing code after a set
//! period of time.
//! 
//! * The [`interval`] function enables code to asycnhronously wait on a
//!   sequence of instants with a fixed period.
//! * The [`sleep`] function enables code to asynchronously wait until a
//!   duration has elapsed.
//! 
//! # Examples
//! 
//! Sleeps 1 second before printing "Hello, World!".
//! 
//! ```no_run
//! 
//! use spdk::time;
//! 
//! #[spdk::main]
//! async fn main() {
//!     print!("Hello, ");
//!     io::stdout().flush().unwrap();
//! 
//!     time::sleep(Duration::from_secs(1)).await;
//! 
//!     println!("World!");
//! }
//! 
//! ```
//! 
//! Counts down from 5 to 1, printing each number with a 1 second delay between
//! each.
//! 
//! ```no_run
//! use std::{time::Duration, io::{self, Write}};
//!
//! use spdk::time::interval;
//! 
//! #[spdk::main]
//! async fn main() {
//!     let mut timer = interval(Duration::from_secs(1));
//! 
//!     for countdown in (1..=5).rev() {
//!         print!("{}...", countdown);
//!         io::stdout().flush().unwrap();
//! 
//!         timer.tick().await;
//! 
//!         print!("\x08\x08\x08\x08");
//!     }
//! 
//!     println!("Hello, World!");
//! }
//! 
//! ```
//! 
use std::{
    cell::UnsafeCell,
    option::Option,
    os::raw::c_void,
    ptr::NonNull,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::{
        Duration,
        Instant,
    },
};

use futures::future::poll_fn;
use spdk_sys::{
    SPDK_POLLER_BUSY,
    SPDK_POLLER_IDLE,

    spdk_poller,

    spdk_poller_register,
    spdk_poller_unregister,
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
    poller: NonNull<spdk_poller>,
    state: Box<UnsafeCell<IntervalState>>
}

impl Interval {
    /// Completes when the next instant in the interval has been reached.
    pub async fn tick(&mut self) -> Instant {
        let tick_fut = poll_fn(move |ctx| self.poll_tick(ctx));

        tick_fut.await
    }

    /// Polls for the next instant in the interval to be reached.
    fn poll_tick(&mut self, ctx: &mut Context<'_>) -> Poll<Instant> {
        let inner = self.state.get_mut();

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
        unsafe { spdk_poller_unregister(&mut self.poller.as_ptr()) }
    }
}

unsafe impl Send for Interval {}

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
        let inner = &mut *(ctx as *mut IntervalState);

        inner.ticker += 1;
        
        if let Some(waker) = inner.waker.take() {
            waker.wake();
            return SPDK_POLLER_BUSY.try_into().unwrap();
        }

        SPDK_POLLER_IDLE.try_into().unwrap()
    }

    let period_us = period.as_micros().try_into().unwrap();
    let state = Box::new(UnsafeCell::new(IntervalState {
            ticker: 0,
            waker: None,
        }));
    let poller = NonNull::new(unsafe {
        spdk_poller_register(
            Some(poll),
            state.get() as *mut c_void,
            period_us
        )
    }).unwrap();

    Interval { poller, state }
}

/// Waits until `duration` has elapsed.
pub async fn sleep(duration: Duration) {
    _ = interval(duration).tick().await;
}
