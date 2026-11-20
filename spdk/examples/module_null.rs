//! # NullRs BVDev Module
//!
//! A custom block device module that ignores write requests and returns zeroed blocks for reads.
use std::io::Write;

use spdk::{
    bdev::{BDev, BDevImpl, BDevOps, IoChannelImpl, IoRequest, IoType, ModuleOps, Owned},
    dma,
    errors::Errno,
    thread,
};

/// Implements the NullRs block device.
///
/// This device ignores write requests and returns zeroed blocks for read requests.
#[derive(Default)]
struct NullRs;

unsafe impl Send for NullRs {}
unsafe impl Sync for NullRs {}

impl NullRs {
    const BLOCK_SIZE: u32 = 4096;
    const NUM_BLOCKS: u64 = 65536;

    /// Creates a new NullRs block device.
    pub async fn new_device() -> spdk::Result<BDev<Owned>> {
        NullRsModule::new_bdev_builder(c"null-rs", Self::BLOCK_SIZE, Self::NUM_BLOCKS)
            .build()
            .await
    }
}

impl BDevOps for NullRs {
    type ChannelContext = ();
    type IoContext = ();
    type IoError = Errno;

    async fn destruct(&mut self) -> spdk::Result<()> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        matches!(io_type, IoType::Read | IoType::Write)
    }

    fn new_io_channel(&mut self) -> spdk::Result<()> {
        Ok(())
    }

    async fn handle_io(
        _this: &BDevImpl<Self>,
        _channel: &IoChannelImpl<()>,
        io: &mut IoRequest<()>,
    ) -> spdk::Result<()> {
        if io.io_type() == IoType::Read {
            let dst = io.buffers_mut();

            dst[0].fill(0);
        }

        Ok(())
    }
}

/// Implements the NullRs block device module.
#[spdk::module(product_name = "Null Disk")]
#[derive(Debug, Default)]
struct NullRsModule;

impl ModuleOps for NullRsModule {
    type BDev = NullRs;
}

/// A program that creates and writes to the NullRs block device.
#[spdk::main]
async fn main() {
    let null = NullRs::new_device().await.unwrap();

    thread::spawn_local(async {
        // Open the NullRs block device in a separate asynchronous task to scope the lifetime of
        // the descriptor and I/O channel. These must be dropped before the NullRs block device can
        // be destroyed.
        let desc = null.open(true).unwrap();
        let ch = desc.io_channel().unwrap();
        let layout = null.layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout).unwrap();

        write!(buf.cursor_mut(), "Hello, World!").unwrap();

        ch.write_at(&desc, &buf, 0).await.unwrap();
    })
    .await;

    null.destroy().await.unwrap();
}
