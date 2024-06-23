use std::{
    ffi::CStr,
    io::{
        Read,
        Write,
    },
};

use byte_strings::c_str;
use spdk::{
    bdev::malloc,
    dma,
    thread,
};

const BDEV_NAME: &CStr = c_str!("Malloc0");
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const DATA: &str = "Hello, World!";

#[spdk::main]
async fn main() {
    // Create a new Malloc block device.
    let malloc = malloc::Builder::new()
        .with_name(BDEV_NAME)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let devname = malloc.name().to_string_lossy().to_string();

    // Open the underlying block device and spawn an asynchronous task to scope
    // the lifetime of the returned descriptor and I/O channel. These must
    // be dropped before the Malloc block device can be destroyed.
    let desc = malloc.open(true).await.unwrap();

    thread::spawn_local(async move {
        let io_chan = desc.io_channel().unwrap();
        let layout = desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        println!("Writing \"{}\" to {}...", DATA, devname);

        write!(buf.cursor_mut(), "{}", DATA).unwrap();

        io_chan.write_at(&buf, 0).await.unwrap();

        buf.clear();

        io_chan.read_at(&mut buf, 0).await.unwrap();

        let mut read_data = String::new();

        buf.cursor().take(DATA.len() as u64).read_to_string(&mut read_data).unwrap();

        assert_eq!(read_data.as_str(), DATA);

        println!("Read \"{}\" from {}.", read_data, devname);
    }).await;

    // Destroy the Malloc block device.
    malloc.destroy().await.unwrap();
}
