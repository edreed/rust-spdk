//! Useful synchronization primitives based on the Storage Performance Development Kit
//! synchronization primitives.
mod condvar;
mod spinlock;

pub use condvar::Condvar;
pub use spinlock::{SpinLock, SpinLockGuard};
