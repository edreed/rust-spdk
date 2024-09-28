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
    option::Option,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::{
        Duration,
        Instant,
    }
};

use futures::future::poll_fn;

use crate::task::{
    Polled,
    Poller,
};

/// Encapsulates the execution state of an [`Interval`].
struct IntervalInner {
    ticker: u32,
    waker: Option<Waker>
}

impl IntervalInner {
    /// Creates a new [`IntervalInner`] with a period of `period`.
    fn new() -> Self {
        Self {
            ticker: 0,
            waker: None,
        }
    }
}

impl Polled for IntervalInner {
    fn poll(&mut self) -> bool {
        self.ticker += 1;
        
        if let Some(waker) = self.waker.take() {
            waker.wake();
            return true;
        }

        false
    }
}

/// A time interval returned by [`interval`].
/// 
/// [`Interval`] enables you to wait asynchronously on a sequence of instants
/// with a fixed period.
pub struct Interval {
    inner: Poller<IntervalInner>
}

impl Interval {
    /// Completes when the next instant in the interval has been reached.
    pub async fn tick(&mut self) -> Instant {
        let tick_fut = poll_fn(move |ctx| self.poll_tick(ctx));

        tick_fut.await
    }

    /// Polls for the next instant in the interval to be reached.
    fn poll_tick(&mut self, ctx: &mut Context<'_>) -> Poll<Instant> {
        let inner = self.inner.polled_mut();

        if inner.ticker == 0 {
            inner.waker = Some(ctx.waker().clone());
            return Poll::Pending;
        }

        inner.ticker = 0;

        Poll::Ready(Instant::now())
    }
}

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
    Interval {
        inner: Poller::with_period(IntervalInner::new(), period),
    }
}

/// Waits until `duration` has elapsed.
pub async fn sleep(duration: Duration) {
    _ = interval(duration).tick().await;
}
