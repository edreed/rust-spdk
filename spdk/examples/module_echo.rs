use std::{
    ffi::CStr,
    io::{
        BufRead,
        IoSlice,
        Write,
    },
    mem::transmute,
    sync::Arc,
};

use async_std::sync::{
    Condvar,
    Mutex,
};
use async_trait::async_trait;
use futures::future::join;
use spdk::{
    bdev::{
        BDevIo,
        BDevIoChannelOps,
        BDevOps,
        ModuleInstance,
        ModuleOps,
    },
    block::{
        Device,
        IoType,
        Owned,
    },
    dma,
    errors::{
        Errno,

        ENOTSUP,
    },
    runtime::reactors,
    task::{self},
    thread::Thread,
};

/// Implements the Echo block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct EchoModule;

#[async_trait(?Send)]
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
    reader: Condvar,
    writer: Condvar,
    src_buf: Mutex<Option<&'static [IoSlice<'static>]>>,
}

/// Implements the Echo block device I/O channel. Each read request is paired
/// with a write request. The read request is completed with the data from the
/// write request and the write request completed when read.
struct EchoChannel {
    device: Arc<EchoInner>
}

impl EchoChannel {
    async fn do_read(&self, io: &mut BDevIo<()>) -> Result<(), Errno> {
        let dst_buf = io.buffers_mut();

        if io.offset_blocks() == 0 {
            // Some Virtual BDev read the first block to inspect
            // partition or other metadata to determine whether
            // to claim the device. We imply return a block of
            // zeros to prevent this BDev from claiming the device.
            dst_buf[0].fill(0);

            return Ok(());
        }

        let mut src_buf = self.device.src_buf.lock().await;

        while src_buf.is_none() {
            src_buf = self.device.reader.wait(src_buf).await;
        }

        dst_buf[0].copy_from_slice(&src_buf.take().unwrap()[0]);

        self.device.writer.notify_one();

        Ok(())
    }

    async fn do_write(&self, io: &mut BDevIo<()>) -> Result<(), Errno> {
        let mut src_buf = self.device.src_buf.lock().await;

        while src_buf.is_some() {
            src_buf = self.device.writer.wait(src_buf).await;
        }

        // SAFETY: We will wait for the reader to consume the buffer before returning.
        unsafe { *src_buf = Some(transmute(io.buffers())) };

        self.device.reader.notify_one();

        while src_buf.is_some() {
            src_buf = self.device.writer.wait(src_buf).await;
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl BDevIoChannelOps for EchoChannel {
    type IoContext = ();

    async fn submit_request(&self, io: &mut BDevIo<Self::IoContext>) -> Result<(), Errno> {
        match io.io_type() {
            IoType::Read => self.do_read(io).await,
            IoType::Write => self.do_write(io).await,
            _ => Err(ENOTSUP)
        }
    }
}

/// Implements the Echo block device.
struct Echo {
    inner: Arc<EchoInner>
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

    fn new_io_channel(&mut self) -> Result<EchoChannel, Errno> {
        let device = self.inner.clone();

        Ok(EchoChannel{ device })
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

    let echo = Echo::try_new(c"echo").unwrap();

    let echo_writer = echo.borrow();

    let write_thread = Thread::new(c"write", &reactors[0].core().into()).unwrap();
    let write_task = write_thread.spawn(move || async move {
        let writer = echo_writer.open(true).await.unwrap();
        let writer_ch = writer.io_channel().unwrap();
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

    let read_thread = Thread::new(c"read", &reactors[1].core().into()).unwrap();
    let read_task = read_thread.spawn(move || async move {
        let reader = echo_reader.open(true).await.unwrap();
        let reader_ch = reader.io_channel().unwrap();
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
