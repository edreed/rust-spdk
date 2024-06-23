use std::{
    ffi::CStr,
    io::{
        BufRead,
        Write
    },
    sync::Arc,
};

use async_trait::async_trait;
use byte_strings::c_str;
use futures::{
    future::join,
    lock::Mutex,
};
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
        Device,
        Owned,
    },
    dma,
    errors::Errno,
    runtime::reactors,
    task::{self},
};

/// Implements the Echo block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct EchoModule {

}

unsafe impl Send for EchoModule {}
unsafe impl Sync for EchoModule {}

#[async_trait]
impl ModuleOps for EchoModule {
    type IoContext = ();

    async fn init(&self) {
        task::yield_now().await;
        println!("Echo module initialized.");
    }

    async fn fini(&self) {
        task::yield_now().await;
        println!("Echo module finalized.");
    }
}

/// State shared among all I/O channels of an Echo block device.
#[derive(Default)]
struct EchoInner {
    pending_write: Option<BDevIo<()>>,
    pending_read: Option<BDevIo<()>>
}

/// Implements the Echo block device I/O channel. Each read request is paired
/// with a write request. The read request is completed with the data from the
/// write request and the write request completed when read.
#[derive(Default)]
struct EchoChannel {
    device: Arc<Mutex<EchoInner>>
}

#[async_trait]
impl BDevIoChannelOps for EchoChannel {
    type IoContext = ();

    async fn submit_request(&self, io: BDevIo<Self::IoContext>) {
        let device = self.device.clone();

        let (read_io, write_io, status) = {
            let mut device = device.lock().await;
            match io.io_type() {
                IoType::Read => {
                    if io.offset_blocks() == 0 {
                        // Some Virtual BDev read the first block to inspect
                        // partition or other metadata to determine whether
                        // to claim the device. We imply return a block of
                        // zeros to prevent this BDev from claiming the device.
                        let dst = io.buffers_mut();

                        dst[0].fill(0);

                        (Some(io), None, IoStatus::Success)
                    } else if device.pending_read.is_some() {
                        (Some(io), None, IoStatus::NoMem)
                    } else if device.pending_write.is_none() {
                        device.pending_read = Some(io);
                        (None, None, IoStatus::Pending)
                    } else {
                        let dst = io.buffers_mut();
                        let write_io = device.pending_write.take().unwrap();
                        let src = write_io.buffers();

                        dst[0].copy_from_slice(&src[0]);

                        (Some(io), Some(write_io), IoStatus::Success)
                    }
                },
                IoType::Write => {
                    if io.offset_blocks() == 0 {
                        // Ignore writes to block 0.
                        (None, Some(io), IoStatus::Success)
                    } else if device.pending_write.is_some() {
                        (None, Some(io), IoStatus::NoMem)
                    } else if device.pending_read.is_none() {
                        device.pending_write = Some(io);
                        (None, None, IoStatus::Pending)
                    } else {
                        let src: &[std::io::IoSlice<'_>] = io.buffers();
                        let read_io = device.pending_read.take().unwrap();
                        let dst = read_io.buffers_mut();

                        dst[0].copy_from_slice(&src[0]);

                        (Some(read_io), Some(io), IoStatus::Success)
                    }
                },
                _ => unreachable!("unexpected IoType value")
            }
        };

        if let Some(read_io) = read_io {
            read_io.complete(status);
        }

        if let Some(write_io) = write_io {
            write_io.complete(status)
        }
    }
}

/// Implements the Echo block device.
struct Echo {
    inner: Arc<Mutex<EchoInner>>
}

impl Echo {
    fn try_new(name: &CStr) -> Result<Device<Owned>, Errno> {
        let mut echo = EchoModule::new_bdev(name, Self::default());

        echo.bdev.blocklen = 4096;
        echo.bdev.blockcnt = 2;

        echo.register()?;

        Ok(echo.into_device())
    }
}

impl Default for Echo {
    fn default() -> Self {
        Self { inner: Arc::new(Default::default()) }
    }
}

unsafe impl Send for Echo {}
unsafe impl Sync for Echo {}

#[async_trait]
impl BDevOps for Echo {
    type IoChannel = EchoChannel;

    async fn destruct(&mut self) -> Result<(), Errno> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        match io_type {
            IoType::Read | IoType::Write => true,
            _ => false
        }
    }

    fn prepare_io_channel(&mut self, channel: &mut EchoChannel) {
        channel.device = self.inner.clone();
    }
}

/// A program the creates an Echo device and writes to it from one reactor and
/// reads from it from another reactor.
#[spdk::main]
async fn main() {
    let reactors: Vec<_> = reactors().collect();

    if reactors.len() < 2 {
        panic!("ERROR: At least two cores must be specified.")
    }

    let echo = Echo::try_new(c_str!("echo")).unwrap();

    let echo_writer = echo.borrow();

    let write_task = reactors[0].spawn(async move {
        let writer = echo_writer.open(true).await.unwrap();
        let mut writer_ch = writer.io_channel().unwrap();
        let layout = writer.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        for i in 0..10 {
            println!("Writing \"Hello {}\"...", i);

            write!(buf.cursor_mut(), "Hello {}", i).unwrap();

            writer_ch.write_blocks_at(&buf, 1).await.unwrap();
        }

        println!("Write complete.");
    });

    let echo_reader = echo.borrow();

    let read_task = reactors[1].spawn(async move {
        let reader = echo_reader.open(true).await.unwrap();
        let mut reader_ch = reader.io_channel().unwrap();
        let layout = reader.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        for _ in 0..10 {
            reader_ch.read_blocks_at(&mut buf, 1).await.unwrap();

            let mut read_data: Vec<u8> = Vec::new();

            let _ = buf.cursor().read_until(b'\0', &mut read_data).unwrap();

            println!("Read \"{}\"", String::from_utf8(read_data).unwrap())
        }

        println!("Read complete.");
    });

    join(write_task, read_task).await;

    echo.destroy().await.unwrap();
}
