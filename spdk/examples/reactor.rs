use std::time::Duration;

use futures::future::join_all;
use spdk::{
    runtime::{
        Reactor,

        reactors,
    },
    time,
};

#[spdk::main]
async fn main() {
    // Spawn a task on each reactor.
    let tasks = reactors()
        .map(
            |r| {
                r.spawn(|| async {
                    let core_id = Reactor::current().core().id();

                    time::sleep(Duration::from_secs(1 * core_id as u64)).await;

                    println!("Hello, World from the reactor on core {}!", core_id);
                })
            });

    // Wait for all tasks to complete.
    join_all(tasks).await;
}
