use std::{time::Duration, io::{self, Write}};

use spdk::time;

#[spdk::main]
async fn main() {
    print!("Hello, ");
    io::stdout().flush().unwrap();

    time::sleep(Duration::from_secs(1)).await;
    
    println!("World!");
}
