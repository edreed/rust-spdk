use spdk::{runtime, task};

fn main() {
    let rt = runtime::Runtime::new();

    rt.block_on(async {
        print!("Hello, ");
        task::yield_now().await;
        println!("World!");
    });
}
