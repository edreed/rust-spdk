use std::{
    cell::OnceCell,
    ffi::{CStr, CString},
    io::{Read, Write},
    slice::{self},
    time::Duration,
};

use libc::EINVAL;
use spdk::{
    bdev::{BDevImpl, BDevIo, BDevIoChannel, BDevOps, ModuleInstance, ModuleOps, malloc},
    block::{
        self, AsRawBDev, ClaimType, Descriptor, Device, EventHandler, EventType, IoChannel,
        IoError, IoResult, IoType, Owned,
    },
    dma::{self},
    errors::ENOTSUP,
    thread, time,
};

/// Implements the PassthruRs block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct PassthruRsModule;

impl ModuleOps for PassthruRsModule {
    type BDev = PassthruRs<'static>;
}

/// Implements the PassthruRs block device per-thread I/O channel context.
struct PassthruRsChannel {
    ch: IoChannel,
}

/// Implements the PassthruRs block device.
///
/// This device passes through I/O requests to an underlying block device.
#[derive(Default)]
struct PassthruRs<'a> {
    desc: OnceCell<Descriptor<'a, Self>>,
}

unsafe impl Send for PassthruRs<'_> {}
unsafe impl Sync for PassthruRs<'_> {}

impl PassthruRs<'_> {
    /// Returns the descriptor for the underlying block device.
    fn desc(&self) -> &Descriptor<'_, Self> {
        self.desc
            .get()
            .expect("PassthruRs descriptor not initialized")
    }
}

impl BDevOps for PassthruRs<'_> {
    type ChannelContext = PassthruRsChannel;
    type IoContext = ();
    type IoError = IoError;

    async fn destruct(&mut self) -> spdk::Result<()> {
        Ok(())
    }

    fn io_type_supported(&self, io_type: IoType) -> bool {
        self.desc().device().io_type_supported(io_type)
    }

    fn new_io_channel(&mut self) -> spdk::Result<Self::ChannelContext> {
        Ok(Self::ChannelContext {
            ch: self.desc().io_channel()?,
        })
    }

    async fn handle_io(
        this: &BDevImpl<Self>,
        channel: &BDevIoChannel<PassthruRsChannel>,
        io: &mut BDevIo<()>,
    ) -> IoResult<()> {
        let base_desc = this.ctx().desc();
        let base_ch = &channel.ctx().ch;

        match io.io_type() {
            IoType::Read => {
                let num_blocks = io.num_blocks();
                let block_size = io.device().logical_block_size() as u64;
                let offset_blocks = io.offset_blocks();

                io.allocate_buffers(num_blocks * block_size).await?;
                base_ch
                    .read_vectored_blocks_at(base_desc, io.buffers_mut(), offset_blocks, num_blocks)
                    .await
            }
            IoType::Write => {
                base_ch
                    .write_vectored_blocks_at(
                        base_desc,
                        io.buffers(),
                        io.offset_blocks(),
                        io.num_blocks(),
                    )
                    .await
            }
            IoType::Unmap => {
                base_ch
                    .unmap_blocks(base_desc, io.offset_blocks(), io.num_blocks())
                    .await
            }
            IoType::Flush => {
                base_ch
                    .flush(base_desc, io.offset_blocks(), io.num_blocks())
                    .await
            }
            IoType::Reset => base_ch.reset(base_desc).await,
            IoType::WriteZeros => {
                base_ch
                    .write_zeroes_blocks_at(base_desc, io.offset_blocks(), io.num_blocks())
                    .await
            }
            IoType::Copy => {
                base_ch
                    .copy_blocks(
                        base_desc,
                        io.copy_source_offset_blocks(),
                        io.offset_blocks(),
                        io.num_blocks(),
                    )
                    .await
            }
            _ => Err(IoError::GeneralError(ENOTSUP)),
        }
    }
}

impl EventHandler for PassthruRs<'_> {
    fn handle_event(&self, event: EventType, _device: &Device<block::OwnedBy<'_, Self>>) {
        if matches!(event, EventType::Remove) {
            todo!();
        }
    }
}

impl<'a> AsRef<PassthruRs<'a>> for PassthruRs<'a> {
    fn as_ref(&self) -> &PassthruRs<'a> {
        self
    }
}

impl PassthruRs<'_> {
    pub async fn new_device<T>(base: &Device<T>) -> spdk::Result<Device<Owned>>
    where
        T: AsRawBDev,
    {
        let name = CString::new(format!("passthru-rs-{}", base.name().to_string_lossy()))
            .map_err(|_| EINVAL)?;

        PassthruRsModule::new_bdev_builder(
            &name,
            base.logical_block_size(),
            base.logical_block_count(),
        )
        .with_write_cache_present(base.has_write_cache())
        .with_buffer_alignment(base.buffer_alignment())
        .with_optimal_io_boundary(base.optimal_io_boundary())
        .with_metadata(base.metadata_size(), base.is_metadata_interleaved())
        .with_dif(
            base.dif_type(),
            base.dif_pi_format(),
            base.is_dif_head_of_metadata(),
            base.dif_check_flags(),
        )
        .build_with_context_in_place(async |mut ctx| {
            let passthru = ctx.write(PassthruRs::default());
            let desc = base.claim(
                ClaimType::ReadManyWriteOne(&name),
                PassthruRsModule::instance(),
                &passthru,
            )?;

            passthru.desc.set(desc).map_err(|_| EINVAL)?;

            Ok(())
        })
        .await
    }
}

const BDEV_NAME: &CStr = c"Malloc0";
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
        .await
        .unwrap()
        .into_owned();

    // The following sleep is a workaround for the lvol BDev which opens a read/write descriptor
    // during examination but completes examination before closing the descriptor.
    time::sleep(Duration::from_secs(1)).await;

    // Create the Passthru block device.
    let passthru = PassthruRs::new_device(&malloc).await.unwrap();

    let devname = passthru.name().to_string_lossy().to_string();

    thread::spawn_local(async {
        let passthru_desc = passthru.open(true).unwrap();
        let io_chan = passthru_desc.io_channel().unwrap();
        let layout = passthru_desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        println!("Writing \"{}\" to {}...", DATA, devname);

        write!(buf.cursor_mut(), "{}", DATA).unwrap();

        io_chan
            .write_vectored_at(
                &passthru_desc,
                slice::from_ref(buf.as_ref()),
                0,
                buf.size() as u64,
            )
            .await
            .unwrap();

        buf.clear();

        let size = buf.size();
        io_chan
            .read_vectored_at(
                &passthru_desc,
                slice::from_mut(buf.as_mut()),
                0,
                size as u64,
            )
            .await
            .unwrap();

        let mut read_data = String::new();

        buf.cursor()
            .take(DATA.len() as u64)
            .read_to_string(&mut read_data)
            .unwrap();

        assert_eq!(read_data.as_str(), DATA);

        println!("Read \"{}\" from {}.", read_data, devname);
    })
    .await;

    // Destroy the Passthru block device.
    passthru.destroy().await.unwrap();

    // Destroy the Malloc block device.
    malloc.destroy().await.unwrap();
}
