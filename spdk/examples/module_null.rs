use std::io::Write;

use async_trait::async_trait;
use spdk::{
    bdev::{BDevIo, BDevIoChannelOps, BDevOps, ModuleInstance, ModuleOps},
    block::{Device, IoType, Owned},
    dma,
    errors::Errno,
};

/// Implements the NullRs block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct NullRsModule;

#[async_trait(?Send)]
impl ModuleOps for NullRsModule {
    type IoContext = ();
}

/// Implements the NullRs block device I/O channel. It ignores write requests
/// and returns zeroed buffers for read requests.
struct NullRsChannel;

#[async_trait(?Send)]
impl BDevIoChannelOps for NullRsChannel {
    type IoContext = ();

    async fn submit_request(&self, io: &mut BDevIo<Self::IoContext>) -> Result<(), Errno> {
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
    pub fn try_new() -> Result<Device<Owned>, Errno> {
        let mut null = NullRsModule::new_bdev(c"null-rs", NullRs);

        null.bdev.blocklen = 4096;
        null.bdev.blockcnt = 1;

        null.register()?;

        Ok(null.into_device())
    }
}

#[async_trait]
impl BDevOps for NullRs {
    type IoChannel = NullRsChannel;

    async fn destruct(&mut self) -> Result<(), Errno> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        matches!(io_type, IoType::Read | IoType::Write)
    }

    fn new_io_channel(&mut self) -> Result<NullRsChannel, Errno> {
        Ok(NullRsChannel)
    }
}

/// A program that creates and writes to the NullRs block device.
#[spdk::main]
async fn main() {
    let null = NullRs::try_new().unwrap();
    let desc = null.open(true).await.unwrap();
    let ch = desc.io_channel().unwrap();
    let layout = null.layout_for_blocks(1).unwrap();
    let mut buf = dma::Buffer::new_zeroed(layout);

    write!(buf.cursor_mut(), "Hello, World!").unwrap();

    ch.write_at(&buf, 0).await.unwrap();

    drop(ch);
    drop(desc);
    null.destroy().await.unwrap();
}
