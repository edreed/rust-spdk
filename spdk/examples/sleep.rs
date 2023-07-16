use std::{time::Duration, io::{self, Write}};

use spdk::{runtime, time};

fn main() {
    let rt = runtime::Runtime::from_cmdline().unwrap();

    rt.block_on(async {
        print!("Hello, ");
        io::stdout().flush().unwrap();
        time::sleep(Duration::from_secs(1)).await;
        println!("World!");
    });
}
