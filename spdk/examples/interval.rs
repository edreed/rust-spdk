use std::{time::Duration, io::{self, Write}};

use spdk::{runtime, time::interval};

#[spdk::main]
async fn main() {
    let mut timer = interval(Duration::from_secs(1));

    for countdown in (1..=5).rev() {
        print!("{}...", countdown);
        io::stdout().flush().unwrap();
        timer.tick().await;
        print!("\x08\x08\x08\x08");
    }

    println!("Hello, World!");
}
