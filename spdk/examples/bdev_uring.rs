use std::{
    ffi::CStr,
    fs::File,
    io::{Read, Write},
};

use spdk::{bdev::uring, dma, thread};

const BDEV_NAME: &CStr = c"Uring0";
const FILENAME: &CStr = c"/tmp/uring0.img";
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const DATA: &str = "Hello, World!";

fn create_file() {
    let uring_file = File::create(FILENAME.to_str().unwrap()).unwrap();

    uring_file.set_len(NUM_BLOCKS * BLOCK_SIZE as u64).unwrap();
}

#[spdk::main]
async fn main() {
    // Create the backing file for the io_uring block device.
    create_file();

    // Create a new io_uring block device.
    let uring = uring::Builder::new()
        .with_name(BDEV_NAME)
        .with_filename(FILENAME)
        .with_block_size(BLOCK_SIZE)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();

    let devname = uring.name().to_string_lossy().to_string();

    thread::spawn_local(async {
        // Open the underlying block device in a separate asynchronous task to scope the lifetime of
        // the descriptor and I/O channel. These must be dropped before the Uring block device can
        // be destroyed.
        let desc = uring.open(true).await.unwrap();
        let mut io_chan = desc.io_channel().unwrap();
        let layout = desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        println!("Writing \"{}\" to {}...", DATA, devname);

        write!(buf.cursor_mut(), "{}", DATA).unwrap();

        io_chan.write_at(&buf, 0).await.unwrap();

        buf.clear();

        io_chan.read_at(&mut buf, 0).await.unwrap();

        let mut read_data = String::new();

        buf.cursor()
            .take(DATA.len() as u64)
            .read_to_string(&mut read_data)
            .unwrap();

        assert_eq!(read_data.as_str(), DATA);

        println!("Read \"{}\" from {}.", read_data, devname);
    })
    .await;

    // Destroy the io_uring block device.
    uring.destroy().await.unwrap();
}
