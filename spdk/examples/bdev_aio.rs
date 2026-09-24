use std::{
    ffi::CStr,
    fs::File,
    io::{Read, Write},
};

use spdk::{bdev, dma, thread};

const BDEV_NAME: &CStr = c"Aio0";
const FILENAME: &str = "/tmp/aio0.img";
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const DATA: &str = "Hello, World!";

fn create_file() {
    let aio_file = File::create(FILENAME).unwrap();

    aio_file.set_len(NUM_BLOCKS * BLOCK_SIZE as u64).unwrap();
}

#[spdk::main]
async fn main() {
    // Create the backing file for the AIO block device.
    create_file();

    // Create a new AIO block device.
    let aio = bdev::Aio::new(
        BDEV_NAME,
        FILENAME,
        Some(BLOCK_SIZE),
        false,
        false,
        None,
        false,
    )
    .unwrap();

    let devname = aio.name().to_string_lossy().to_string();

    // Open the underlying block device and spawn an asynchronous task to scope
    // the lifetime of the returned descriptor and I/O channel. These must
    // be dropped before the AIO block device can be destroyed.
    let desc = aio.open(true).await.unwrap();

    thread::spawn_local(async move {
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

    // Destroy the AIO block device.
    aio.destroy().await.unwrap();
}
