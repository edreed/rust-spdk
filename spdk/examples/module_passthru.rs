use std::{
    ffi::{
        CStr,
        CString,
    },
    io::{
        Read,
        Write,
    },
    option::Option,
    slice::{self},
};

use async_trait::async_trait;
use byte_strings::c_str;
use spdk::{
    bdev::{
        BDevIo,
        BDevIoChannelOps,
        BDevOps,
        IoType,
        malloc,
        ModuleInstance,
        ModuleOps
    },
    block::{
        Any,
        Descriptor,
        Device,
        IoChannel,
        Owned,
    },
    dma::{
        self,
    },
    errors::{
        Errno,

        ENOTSUP,
    },
    thread,
};

#[spdk::module]
#[derive(Debug, Default)]
struct PassthruRsModule;

impl ModuleOps for PassthruRsModule {
    type IoContext = ();
}

#[derive(Debug, Default)]
struct PassthruRsChannel {
    ch: Option<IoChannel>
}

impl PassthruRsChannel {
    fn channel(&self) -> &IoChannel {
        &self.ch.as_ref().unwrap()
    }
}

#[async_trait]
impl BDevIoChannelOps for PassthruRsChannel {
    type IoContext = ();

    async fn submit_request(&self, io: BDevIo<Self::IoContext>) {
        let res = match io.io_type() {
            spdk::bdev::IoType::Read => {
                self.channel().read_vectored_blocks_at(io.buffers_mut(), io.offset_blocks(), io.num_blocks()).await
            },
            spdk::bdev::IoType::Write => {
                self.channel().write_vectored_blocks_at(io.buffers(), io.offset_blocks(), io.num_blocks()).await
            },
            spdk::bdev::IoType::Unmap => {
                self.channel().unmap_blocks(io.offset_blocks(), io.num_blocks()).await
            },
            spdk::bdev::IoType::Flush => {
                self.channel().flush(io.offset_blocks(), io.num_blocks()).await
            },
            spdk::bdev::IoType::Reset => {
                self.channel().reset().await
            },
            spdk::bdev::IoType::WriteZeros => {
                self.channel().write_zeroes_blocks_at(io.offset_blocks(), io.num_blocks()).await
            },
            spdk::bdev::IoType::Copy => {
                self.channel().copy_blocks(io.copy_source_offset_blocks(), io.offset_blocks(), io.num_blocks()).await
            },
            _ => Err(ENOTSUP),
        };

        io.complete(res.into());
    }
}

struct PassthruRs {
    base: Device<Any>,
    desc: Descriptor
}

unsafe impl Send for PassthruRs {}
unsafe impl Sync for PassthruRs {}

#[async_trait]
impl BDevOps for PassthruRs {
    type IoChannel = PassthruRsChannel;
    
    async fn destruct(&mut self) ->  Result<(), Errno> {
        Ok(())
    }
    
    fn io_type_supported(&self, io_type: IoType) -> bool {
        self.base.io_type_supported(io_type)
    }

    fn prepare_io_channel(&mut self, channel: &mut Self::IoChannel) {
        let ch = self.desc.io_channel().unwrap();

        channel.ch = Some(ch);
    }
}

impl PassthruRs {
    pub fn try_new(base: Device<Any>, desc: Descriptor) -> Result<Device<Owned>, Errno> {
        let name = CString::new(format!("passthru-rs-{}", base.name().to_string_lossy())).unwrap();
        let mut passthru = PassthruRsModule::new_bdev(name.as_c_str(), PassthruRs { base, desc });

        let base = unsafe{ &mut *passthru.ctx().base.as_ptr() };

        passthru.bdev.write_cache = base.write_cache;
        passthru.bdev.required_alignment = base.required_alignment;
        passthru.bdev.optimal_io_boundary = base.optimal_io_boundary;
        passthru.bdev.blocklen = base.blocklen;
        passthru.bdev.blockcnt = base.blockcnt;

        passthru.bdev.md_interleave = base.md_interleave;
        passthru.bdev.md_len = base.md_len;
        passthru.bdev.dif_type = base.dif_type;
        passthru.bdev.dif_is_head_of_md = base.dif_is_head_of_md;
        passthru.bdev.dif_check_flags = base.dif_check_flags;

        passthru.register()?;

        Ok(passthru.into_device())
    }
}

const BDEV_NAME: &CStr = c_str!("Malloc0");
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
        .unwrap()
        .into_owned()
        .unwrap();
    let malloc_desc = malloc.open(true).await.unwrap();

    // Create the Passthru block device.
    let passthru = PassthruRs::try_new(malloc.borrow(), malloc_desc).unwrap();
    let passthru_desc = passthru.open(true).await.unwrap();
    
    let devname = passthru.name().to_string_lossy().to_string();

    thread::spawn_local(async move {
        let io_chan = passthru_desc.io_channel().unwrap();
        let layout = passthru_desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout);

        println!("Writing \"{}\" to {}...", DATA, devname);

        write!(buf.cursor_mut(), "{}", DATA).unwrap();

        io_chan.write_vectored_at(slice::from_ref(buf.as_ref()), 0, buf.size() as u64).await.unwrap();

        buf.clear();

        let size = buf.size();
        io_chan.read_vectored_at(slice::from_mut(buf.as_mut()), 0, size as u64).await.unwrap();

        let mut read_data = String::new();

        buf.cursor().take(DATA.len() as u64).read_to_string(&mut read_data).unwrap();

        assert_eq!(read_data.as_str(), DATA);

        println!("Read \"{}\" from {}.", read_data, devname);
    }).await;

    // Destroy the Passthru block device.
    passthru.destroy().await.unwrap();

    // Destroy the Malloc block device.
    malloc.destroy().await.unwrap();
}
