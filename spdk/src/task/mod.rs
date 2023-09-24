//! Asynchronous task management for Storage Performance Development Kit [Event
//! Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
mod join_handle;
mod task;
mod yield_now;

pub(crate) use task::{
    RcTask,
    Task,
};

pub use join_handle::JoinHandle;
pub use yield_now::yield_now;