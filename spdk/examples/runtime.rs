use spdk::runtime;

fn main() {
    let rt = runtime::Runtime::new();

    rt.block_on(async {
        println!("Hello, World!");
    });
}