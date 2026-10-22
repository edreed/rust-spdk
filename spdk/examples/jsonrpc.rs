use std::time::Duration;

use spdk::{
    Result,
    errors::ERANGE,
    jsonrpc,
    runtime::{CpuSet, Runtime},
    thread,
    time::interval,
};

/// Synchronously prints a greeting.
#[jsonrpc]
fn hello_world() {
    println!("HELLO, WORLD!");
}

/// Synchronously prints the message from the requestor.
#[jsonrpc]
fn print_message(msg: &str) {
    println!("MESSAGE: {}", msg);
}

/// Asynchronously adds two integers and returns the result.
#[jsonrpc]
async fn add_two(a: i32, b: i32) -> i32 {
    thread::spawn_local(async { a + b }).await
}

/// Plays a guessing game asynchronously with the requestor.
#[jsonrpc]
async fn guess_number(guess: u8) -> Result<String> {
    thread::spawn(c"guess_number", &CpuSet::any(), || async {
        if !(1..=10).contains(&guess) {
            return Err(ERANGE);
        }

        if guess == 5 {
            return Ok("CORRECT!".to_string());
        }

        Ok("WRONG!".to_string())
    })?
    .await
}

#[spdk::main]
async fn main() {
    let mut timer = interval(Duration::from_millis(50));

    while !Runtime::is_shutting_down() {
        timer.tick().await;
    }
}
