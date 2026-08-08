use std::ffi::CStr;

use futures::StreamExt;

use spdk::{
    bdev::malloc,
    block::{self, Device, Owned},
};

const NUM_BLOCKS: u64 = 16536;
const BLOCK_SIZE: u32 = 512;

fn create_bdev(name: &CStr) -> Device<Owned> {
    malloc::Builder::new()
        .with_name(name)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .unwrap()
        .into_owned()
        .unwrap()
}

#[spdk::main]
async fn main() {
    let malloc0 = create_bdev(c"malloc0");
    let malloc1 = create_bdev(c"malloc1");

    let mut devices = block::devices();

    while let Some(dev) = devices.next().await {
        println!(
            "Found \"{}\" {}",
            dev.name().to_string_lossy(),
            dev.product_name().to_string_lossy()
        );
    }

    malloc1.destroy().await.unwrap();
    malloc0.destroy().await.unwrap();
}
