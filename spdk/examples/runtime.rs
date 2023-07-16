use std::io::{self, Write};

use spdk::{runtime, task};

fn main() {
    let rt = runtime::Runtime::from_cmdline().unwrap();

    rt.block_on(async {
        print!("Hello, ");
        io::stdout().flush().unwrap();
        task::yield_now().await;
        println!("World!");
    });
}
