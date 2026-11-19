use std::ffi::CStr;

use spdk::{
    bdev::MallocBuilder,
    bdev::{self, BDev, Owned},
};

const NUM_BLOCKS: u64 = 16536;
const BLOCK_SIZE: u32 = 512;

async fn create_bdev(name: &CStr) -> BDev<Owned> {
    MallocBuilder::new()
        .with_name(name)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .await
        .unwrap()
        .into_owned()
}

#[spdk::main]
async fn main() {
    let malloc0 = create_bdev(c"malloc0").await;
    let malloc1 = create_bdev(c"malloc1").await;

    let bdevs = bdev::all();

    bdevs.for_each(|bdev| {
        println!(
            "Found \"{}\" {}",
            bdev.name().to_string_lossy(),
            bdev.product_name().to_string_lossy()
        );
    });

    malloc1.destroy().await.unwrap();
    malloc0.destroy().await.unwrap();
}
