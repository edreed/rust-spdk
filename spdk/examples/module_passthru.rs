use std::{
    cell::OnceCell,
    collections::HashMap,
    ffi::{CStr, CString},
    io::{Read, Write},
    slice,
    time::Duration,
};

use spdk::{
    bdev::{BDevImpl, BDevIo, BDevIoChannel, BDevOps, Examiner, ModuleInstance, ModuleOps, malloc},
    block::{
        self, AsRawBDev, ClaimType, Descriptor, Device, EventHandler, EventType, IoChannel,
        IoError, IoResult, IoType, Owned,
    },
    dma::{self},
    errors::{EINVAL, ENODEV, ENOTSUP},
    thread::{self, App, ThreadCell},
    time::sleep,
};
use ternary_rs::if_else;

/// Implements the PassthruRs block device module.
#[spdk::module]
#[derive(Debug, Default)]
struct PassthruRsModule {
    bdevs: ThreadCell<App, HashMap<CString, Option<Device<Owned>>>>,
}

impl PassthruRsModule {
    /// Enables the creation of a `PassthruRs` BDev for the specified base BDev by name.
    ///
    /// If the base BDev exists when this method is called, a `PassthruRs` device will be created
    /// and claim the base BDev before returning. If the base BDev does not exist, the `PassthruRs`
    /// device will be created and claim the base BDev in the examination process.
    async fn enable_for_name(&self, base_name: &CStr) -> spdk::Result<()> {
        debug_assert!(
            thread::is_application(),
            "register_for_name must be called from the application thread"
        );

        let base_bdev = Device::from_name(base_name)
            .map(Option::Some)
            .or_else(|e| if_else!(e == ENODEV, Ok(None), Err(e)))?;

        unsafe {
            self.bdevs.with_unchecked(move |bdevs| {
                bdevs.entry(base_name.to_owned()).or_default();
            });
        }

        if let Some(base_bdev) = base_bdev {
            self.create_for_base(&base_bdev).await?;
        }

        Ok(())
    }

    /// Creates an instance of the `PassthruRs` BDev for the specified base BDev.
    async fn create_for_base<A>(&self, base_bdev: &Device<A>) -> spdk::Result<()>
    where
        A: AsRawBDev,
    {
        debug_assert!(
            thread::is_application(),
            "register_for_bdev must be called from the application thread"
        );

        let passthru = PassthruRs::new_device(base_bdev).await?;

        let previous = unsafe {
            self.bdevs.with_unchecked(move |bdevs| {
                bdevs
                    .insert(base_bdev.name().to_owned(), Some(passthru))
                    .unwrap_or(None)
            })
        };

        if let Some(previous) = previous {
            unreachable!(
                "PassthruRs device for {} already registered: {:?}",
                base_bdev.name().to_string_lossy(),
                previous
            );
        }

        Ok(())
    }

    /// Destroys the `PassthruRs` BDev for the base BDev specified by name.
    async fn destroy_for_name(&self, base_name: &CStr) -> spdk::Result<()> {
        debug_assert!(
            thread::is_application(),
            "unregister_for_name must be called from the application thread"
        );

        let passthru = unsafe {
            self.bdevs
                .with_unchecked(move |bdevs| bdevs.remove(base_name).unwrap_or(None))
        };

        if let Some(passthru) = passthru {
            passthru.destroy().await?;
        }

        Ok(())
    }

    /// Returns `true` if the base BDev specified by name should be claimed by a `PassthruRs` BDev.`
    fn should_claim(&self, base_name: &CStr) -> bool {
        debug_assert!(
            thread::is_application(),
            "should_claim must be called from the application thread"
        );

        unsafe {
            self.bdevs
                .with_unchecked(move |bdevs| bdevs.contains_key(base_name))
        }
    }

    /// Returns the name of the `PassthruRs` BDev for the base BDev name.
    fn passthru_name(base_name: &CStr) -> CString {
        CString::new(format!("passthru-rs-{}", base_name.to_string_lossy()))
            .expect("created PassthruRs device name")
    }
}

impl ModuleOps for PassthruRsModule {
    type BDev = PassthruRs<'static>;

    async fn examine_disk<'a>(&self, bdev: Device<block::OwnedBy<'a, Examiner<Self>>>) {
        // The following delay works around a race condition with the lvol VBDev. It opens a
        // read/write descriptor while inspecting the block device for claim suitability. The open
        // read/write descriptor prevents the PassthruRs bdev from from claiming the Malloc bdev.
        // The wait here allows the lvol VBDev to complete its examination and close the read/write
        // descriptor.
        sleep(Duration::from_secs(1)).await;

        if self.should_claim(bdev.name()) {
            self.create_for_base(&bdev).await.expect("bdev registered");
        }
    }
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
            let base_name = self.desc().device().name().to_owned();

            unsafe {
                thread::spawn_local_detached(async move {
                    PassthruRsModule::instance()
                        .ctx()
                        .destroy_for_name(&base_name)
                        .await
                        .unwrap();
                });
            }
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
        let name = PassthruRsModule::passthru_name(base.name());

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
    PassthruRsModule::instance()
        .ctx()
        .enable_for_name(BDEV_NAME)
        .await
        .unwrap();

    // Create a new Malloc block device.
    let malloc = malloc::Builder::new()
        .with_name(BDEV_NAME)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .await
        .unwrap()
        .into_owned();

    thread::spawn_local(async {
        let devname = PassthruRsModule::passthru_name(malloc.name());
        let passthru = Device::from_name(&devname).unwrap();
        let passthru_desc = passthru.open(true).unwrap();
        let io_chan = passthru_desc.io_channel().unwrap();
        let layout = passthru_desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        println!("Writing \"{}\" to {}...", DATA, devname.to_string_lossy());

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

        println!("Read \"{}\" from {}.", read_data, devname.to_string_lossy());
    })
    .await;

    // Destroy the Malloc block device.
    malloc.destroy().await.unwrap();
}
