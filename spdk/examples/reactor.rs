use futures::future::join_all;
use spdk::runtime::{Reactor, reactors};

#[spdk::main]
async fn main() {
    // Spawn a task on each reactor.
    let tasks = reactors().map(|r| {
        r.spawn(|| async {
            let core_id = Reactor::current().core().id();

            println!("Hello, World from the reactor on core {}!", core_id);
        })
    });

    // Wait for all tasks to complete.
    join_all(tasks).await;
}
