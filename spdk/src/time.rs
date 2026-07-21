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
    future::Future,
    option::Option,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use crate::task::{Polled, Poller};

/// Encapsulates the execution state of an [`Interval`].
struct IntervalInner {
    ticker: u32,
    waker: Option<Waker>,
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
    fn poll(mut self: Pin<&mut Self>) -> bool {
        self.ticker += 1;

        if let Some(waker) = self.waker.take() {
            waker.wake();
            return true;
        }

        false
    }
}

impl Future for IntervalInner {
    type Output = Instant;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Instant> {
        if self.ticker == 0 {
            self.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        self.ticker = 0;

        Poll::Ready(Instant::now())
    }
}

/// A time interval returned by [`interval`].
///
/// [`Interval`] enables you to wait asynchronously on a sequence of instants
/// with a fixed period.
pub struct Interval(Poller<IntervalInner>);

impl Interval {
    /// Completes when the next instant in the interval has been reached.
    pub fn tick(&mut self) -> &mut impl Future<Output = Instant> {
        &mut self.0
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
    Interval(Poller::with_period(period, IntervalInner::new()))
}

/// Waits until `duration` has elapsed.
pub async fn sleep(duration: Duration) {
    _ = interval(duration).tick().await;
}
