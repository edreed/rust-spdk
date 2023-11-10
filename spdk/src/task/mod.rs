//! Asynchronous task management for Storage Performance Development Kit [Event
//! Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
mod join_handle;
mod promise;
mod task;
mod yield_now;

#[allow(unused_imports)]
pub(crate) use promise::{
    Promise,

    complete_with_object,
    complete_with_status,
    complete_with_ok,
};
pub(crate) use task::{
    RcTask,
    ReactorTask,
    ThreadTask,
};

pub use join_handle::JoinHandle;
pub use yield_now::yield_now;