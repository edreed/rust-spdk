use std::{
    io::Write,
    ptr::NonNull,
    task::Poll,
};

use async_trait::async_trait;
use byte_strings::c_str;
use spdk::{
    bdev::{
        BDevIo,
        BDevIoChannelOps,
        BDevOps,
        IoStatus,
        IoType,
        ModuleInstance,
        ModuleOps,
    },
    block::{
        Device, Owned, OwnedOps
    },
    dma,
    errors::Errno,
    task::{
        complete_with_status,
        Promise,
    }
};
use spdk_sys::{
    spdk_bdev,

    spdk_bdev_unregister,
};

/// Implements the NullRs block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct NullRsModule;

impl ModuleOps for NullRsModule {
    type IoContext = ();
}

/// Implements the NullRs block device I/O channel. It ignores write requests
/// and returns zeroed buffers for read requests.
#[derive(Debug, Default)]
struct NullRsChannel;

impl BDevIoChannelOps for NullRsChannel {
    type IoContext = ();

    fn submit_request(&self, io: BDevIo<Self::IoContext>) {
        if io.io_type() == IoType::Read {
            let dst = io.buffers_mut();

            dst[0].fill(0);
        }

        io.complete(IoStatus::Success);
    }
}

/// Implements the NullRs block device.
#[derive(Default)]
struct NullRsCtx;

unsafe impl Send for NullRsCtx {}
unsafe impl Sync for NullRsCtx {}

#[async_trait]
impl BDevOps for NullRsCtx {
    type IoChannel = NullRsChannel;

    async fn destruct(&mut self) -> Result<(), Errno> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        matches!(io_type, IoType::Read | IoType::Write)
    }

}

/// Implements the owned device wrapper governing the NullRs device lifetime.
struct NullRs(NonNull<spdk_bdev>);

impl NullRs {
    pub fn try_new() -> Result<Device<Self>, Errno> {
        let mut null = NullRsModule::new_bdev(c_str!("null-rs"), NullRsCtx::default());

        null.bdev.blocklen = 4096;
        null.bdev.blockcnt = 1;

        null.register()?;

        Ok(Device::new(Self(unsafe { NonNull::new_unchecked(null.into_bdev_ptr()) })))
    }
}

unsafe impl Send for NullRs {}
unsafe impl Sync for NullRs {}

impl From<Owned> for NullRs {
    fn from(owned: Owned) -> Self {
        NullRs(unsafe { NonNull::new_unchecked(owned.into_ptr()) })
    }
}

#[async_trait]
impl OwnedOps for NullRs {
    fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_ptr()
    }

    async fn destroy(self) -> Result<(), Errno> {
        Promise::new(move |cx| {
            unsafe {
                spdk_bdev_unregister(
                    self.as_ptr(),
                    Some(complete_with_status),
                    cx);
            }

            Poll::Pending
        }).await
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
}
