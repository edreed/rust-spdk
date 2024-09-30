//! Asynchronous task management for the Storage Performance Development Kit
//! [Event Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
mod join_handle;
mod poller;
mod promise;
mod task;
mod yield_now;

pub(crate) use task::{
    Task,
    ReactorTask,
    ThreadTask,
};

pub use join_handle::JoinHandle;
pub use poller::{
    Polled,
    PolledFn,
    Poller,

    polled_fn,
    polled_fn_with_period,
};
pub use promise::{
    Promise,
    Promissory,
};
pub use yield_now::yield_now;
