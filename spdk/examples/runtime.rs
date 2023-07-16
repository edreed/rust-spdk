use std::io::{self, Write};

use spdk::{runtime, task};

#[spdk::main]
async fn main() {
    print!("Hello, ");
    io::stdout().flush().unwrap();
    task::yield_now().await;
    println!("World!");
}
