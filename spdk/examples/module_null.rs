use std::io::Write;

use spdk::{
    bdev::{BDevIo, BDevIoChannelOps, BDevOps, ModuleOps},
    block::{Device, IoType, Owned},
    dma,
};

/// Implements the NullRs block device module.
#[spdk::module(product_name = "Null Disk")]
#[derive(Debug, Default)]
struct NullRsModule;

impl ModuleOps for NullRsModule {
    type BDev = NullRs;
}

/// Implements the NullRs block device I/O channel. It ignores write requests
/// and returns zeroed buffers for read requests.
struct NullRsChannel;

impl BDevIoChannelOps for NullRsChannel {
    type IoContext = ();

    async fn submit_request(&mut self, io: &mut BDevIo<Self::IoContext>) -> spdk::Result<()> {
        if io.io_type() == IoType::Read {
            let dst = io.buffers_mut();

            dst[0].fill(0);
        }

        Ok(())
    }
}

/// Implements the NullRs block device.
#[derive(Default)]
struct NullRs;

unsafe impl Send for NullRs {}
unsafe impl Sync for NullRs {}

impl NullRs {
    /// Creates a new NullRs block device.
    pub fn try_new() -> spdk::Result<Device<Owned>> {
        NullRsModule::new_bdev_builder(c"null-rs", 4096, 1).build()
    }
}

impl BDevOps for NullRs {
    type IoChannel = NullRsChannel;

    async fn destruct(&mut self) -> spdk::Result<()> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        matches!(io_type, IoType::Read | IoType::Write)
    }

    fn new_io_channel(&mut self) -> spdk::Result<NullRsChannel> {
        Ok(NullRsChannel)
    }
}

/// A program that creates and writes to the NullRs block device.
#[spdk::main]
async fn main() {
    let null = NullRs::try_new().unwrap();
    let desc = null.open(true).await.unwrap();
    let mut ch = desc.io_channel().unwrap();
    let layout = null.layout_for_blocks(1).unwrap();
    let mut buf = dma::Buffer::new_zeroed(layout);

    write!(buf.cursor_mut(), "Hello, World!").unwrap();

    ch.write_at(&buf, 0).await.unwrap();

    drop(ch);
    drop(desc);
    null.destroy().await.unwrap();
}
