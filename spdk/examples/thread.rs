use std::{time::Duration, ffi::CString};

use futures::future::join_all;
use spdk::{
    runtime::reactors,
    time, thread::Thread,
};

#[spdk::main]
async fn main() {
    // Spawn a task on each reactor.
    let tasks = reactors()
        .map(
            |r| {
                let core = r.core();

                r.spawn(move || async move {
                    // Create a new thread on the current reactor's core and
                    // spawn a a task on it.
                    let name = CString::new(format!("thread_{}", core.id())).unwrap();
                    let t = Thread::new(name.as_c_str(), &core.into()).unwrap();

                    t.spawn(move || async move {
                        time::sleep(Duration::from_secs(1 * core.id() as u64)).await;

                        println!("Hello, World from {}!", Thread::current().name().to_string_lossy());
                    }).await
                })
            });

    // Wait for all tasks to complete.
    join_all(tasks).await;
}
