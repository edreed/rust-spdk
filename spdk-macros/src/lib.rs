//! Procedural macros for the spdk crate.
mod cli;
mod main_attr;

#[cfg(feature = "bdev-module")]
mod module;

use proc_macro::TokenStream;

/// Derives the [`Parser`] trait for a struct defining command-line arguments in
/// addition to the standard SPDK Application Framework arguments.
/// 
/// This macro provides simple command-line argument parsing for SPDK
/// applications and integrates with the [`macro@main`] macro. It allows an
/// application to extend the set of command-line arguments supported by the
/// SPDK Event Framework. The derived struct will implement the [`Parser`] trait
/// and its `parse` method can be passed to the [`macro@main`] macro.
/// 
/// If you need more advanced argument parsing, consider using a crate like
/// [`clap`] and create a runtime using the [`Builder`] type.
/// 
/// # Field Types
/// 
/// The type of each field in the derived struct must implement the [`FromStr`]
/// trait.
/// 
/// Boolean fields are treated as flags and will be set to `true` if present on
/// the command-line. The option supports an optional value containing `true` or
/// `false` to explicitly set the value.
/// 
/// # Attributes
/// 
/// The optional `spdk_arg` helper attribute can be used the customize the
/// command-line behavior. The following metadata is supported by this
/// attribute:
/// 
/// * `short`: The short name of the argument, e.g. `short = 'C'`. If not present,
///   no short argument will be generated.
/// * `long`: The long name of the argument, e.g. `long = "block-count"`. If not
///   present, the field name will be converted to a long argument by replacing
///   any underscores (`'_'`) with dashes (`'-'`).
/// * `default`: The default value for the argument, e.g. `default = 512`. If
///   not present, the field type must implement the [`Default`] trait.
/// * `value_name`: The name of the value for the argument that will appear in
///   the usage help text, e.g. `value_name = "SIZE"`. If not present, the value
///   name will be derived from the field name by converting to uppercase and
///   replacing underscores with dashes.
/// 
/// # Help Text
/// 
/// The help text for the derived struct will be generated from the doc comments
/// for each field.
/// 
/// # Usage
/// 
/// ```no_run
/// use std::path::PathBuf;
/// use spdk::{self, cli::Parser};
/// 
/// #[derive(Debug, Parser)]
/// struct Args {
///     /// Path to the block device to use.
///     #[spdk_arg(short = 'D')]
///     device_path: PathBuf,
///
///     /// The block size of the device in bytes.
///     #[spdk_arg(default = 512)]
///     block_size: u32,
///
///     /// If specified, creates a new block device.
///     create_new: bool,
/// }
///
/// #[spdk::main(cli_args = Args::parse())]
/// async fn main() {
///     let args = Args::get();
/// 
///     println!("{:#?}", args);
/// }
/// ```
/// 
/// The following help text will be generated for the above example:
/// 
/// ```text
/// $ cli --help
/// cli [options]
/// options:
///  -c, --config <config>     JSON config file (default none)
///      --json <config>       JSON config file (default none)
///      --json-ignore-init-errors
///                            don't exit on invalid config entry
///  -d, --limit-coredump      do not set max coredump size to RLIM_INFINITY
///  -g, --single-file-segments
///                            force creating just one hugetlbfs file
///  -h, --help                show this usage
///  -i, --shm-id <id>         shared memory ID (optional)
///  -m, --cpumask <mask or list>    core mask (like 0xF) or core list of '[]' embraced (like [0,1,10]) for DPDK
///      --lcores <list>       lcore to CPU mapping list. The list is in the format:
///                            <lcores[@CPUs]>[<,lcores[@CPUs]>...]
///                            lcores and cpus list are grouped by '(' and ')', e.g '--lcores "(5-7)@(10-12)"'
///                            Within the group, '-' is used for range separator,
///                            ',' is used for single number separator.
///                            '( )' can be omitted for single element group,
///                            '@' can be omitted if cpus and lcores have the same value
///  -n, --mem-channels <num>  channel number of memory channels used for DPDK
///  -p, --main-core <id>      main (primary) core for DPDK
///  -r, --rpc-socket <path>   RPC listen address (default /var/tmp/spdk.sock)
///  -s, --mem-size <size>     memory size in MB for DPDK (default: 0MB)
///      --disable-cpumask-locks    Disable CPU core lock files.
///      --silence-noticelog   disable notice level logging to stderr
///      --msg-mempool-size <size>  global message memory pool size in count (default: 262143)
///  -u, --no-pci              disable PCI access
///      --wait-for-rpc        wait for RPCs to initialize subsystems
///      --max-delay <num>     maximum reactor delay (in microseconds)
///  -B, --pci-blocked <bdf>
///                            pci addr to block (can be used more than once)
///  -R, --huge-unlink         unlink huge files after initialization
///  -v, --version             print SPDK version
///  -A, --pci-allowed <bdf>
///                            pci addr to allow (-B and -A cannot be used at the same time)
///      --huge-dir <path>     use a specific hugetlbfs mount to reserve memory from
///      --iova-mode <pa/va>   set IOVA mode ('pa' for IOVA_PA and 'va' for IOVA_VA)
///      --base-virtaddr <addr>      the base virtual address for DPDK (default: 0x200000000000)
///      --num-trace-entries <num>   number of trace entries for each core, must be power of 2, setting 0 to disable trace (default 32768)
///                                  Tracepoints vary in size and can use more than one trace entry.
///      --rpcs-allowed        comma-separated list of permitted RPCS
///      --env-context         Opaque context for use of the env implementation
///      --vfio-vf-token       VF token (UUID) shared between SR-IOV PF and VFs for vfio_pci driver
///  -L, --logflag <flag>    enable log flag (all, app_config, app_rpc, json_util, log, log_rpc, reactor, rpc, rpc_client, thread, trace)
///  -e, --tpoint-group <group-name>[:<tpoint_mask>]
///                            group_name - tracepoint group name for spdk trace buffers (thread, all)
///                            tpoint_mask - tracepoint mask for enabling individual tpoints inside a tracepoint group. First tpoint inside a group can be enabled by setting tpoint_mask to 1 (e.g. bdev:0x1).
///                             Groups and masks can be combined (e.g. thread,bdev:0x1).
///                             All available tpoints can be found in /include/spdk_internal/trace_defs.h
///   -D, --device-path <DEVICE>
///                            Path to the block device to use.
///       --block-size <SIZE>  The block size of the device in bytes.
///       --create-new [CREATE_NEW]
///                            If specified, creates a new block device.
/// ```
/// 
/// [`Parser`]: ../../spdk/cli/trait.Parser.html
/// [`clap`]: https://crates.io/crates/clap
/// [`Builder`]: ../../spdk/runtime/struct.Builder.html
/// [`FromStr`]: https://doc.rust-lang.org/std/str/trait.FromStr.html
/// [`Default`]: https://doc.rust-lang.org/std/default/trait.Default.html
#[proc_macro_derive(Parser, attributes(spdk_arg))]
pub fn parser(input: TokenStream) -> TokenStream {
    cli::DeriveParser::new().derive(input)
}

/// Marks the main entry point of an application using the SPDK Application
/// Framework.
/// 
/// # Notes
/// 
/// This macro is for applications that do not require a complex setup. Consider
/// using [`Builder`](../spdk/runtime/struct.Builder.html) to create a
/// [`Runtime`](../spdk/runtime/struct.Runtime.html) directly if this macro does
/// not meet your needs.
/// 
/// The [`Runtime`](../spdk/runtime/struct.Runtime.html) created by this macro
/// is initialized from the command line arguments supported by the
/// [`spdk_app_parse_args`](../spdk_sys/fn.spdk_app_parse_args.html) function.
/// 
/// # Attributes
/// 
/// The `cli_args` attribute is used with the `Parser` trait to specify the
/// parsing function for a struct defining command-line arguments. See the
/// [`Parser`] derive macro for more information.
/// 
/// # Usage
/// 
/// ```no_run
/// #[spdk::main]
/// async fn main() {
///     println!("Hello, World!");
/// }
/// ```
#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    main_attr::generate_main(attr, item)
}

/// Marks a struct as a SPDK block device module.
/// 
/// This attribute macro creates a singleton instance of type [`Module<T>`] for
/// the target struct and registers it with the SPDK block device module list.
/// 
/// The singleton instance can be accessed using the [`instance()`] method of
/// the [`ModuleInstance`] trait.
/// 
/// BDev implementors call the [`new_bdev()`] method of the `ModuleInstance`
/// trait to create a new block device instance.
/// 
/// # Example
/// 
/// The following example creates a block device module for a device that
/// ignores writes and returns zeroed buffers for reads:
/// 
/// 
/// ```rust
/// use std::{
///     io::Write,
///     ptr::NonNull,
///     task::Poll,
/// };
///
/// use async_trait::async_trait;
/// use byte_strings::c_str;
/// use spdk::{
///     bdev::{
///         BDevIo,
///         BDevIoChannelOps,
///         BDevOps,
///         IoStatus,
///         IoType,
///         ModuleInstance,
///         ModuleOps,
///     },
///     block::{
///         Device, Owned, OwnedOps
///     },
///     dma,
///     errors::Errno,
///     task::{
///         complete_with_status,
///         Promise,
///     }
/// };
/// use spdk_sys::{
///     spdk_bdev,
///
///     spdk_bdev_unregister,
/// };
///
/// /// Implements the NullRs block device module.
/// #[spdk::module]
/// #[derive(Debug, Default)]
/// struct NullRsModule;
///
/// impl ModuleOps for NullRsModule {
///     type IoContext = ();
/// }
///
/// /// Implements the NullRs block device I/O channel. It ignores write requests
/// /// and returns zeroed buffers for read requests.
/// #[derive(Debug, Default)]
/// struct NullRsChannel;
///
/// impl BDevIoChannelOps for NullRsChannel {
///     type IoContext = ();
///
///     fn submit_request(&self, io: BDevIo<Self::IoContext>) {
///         if io.io_type() == IoType::Read {
///             let dst = io.buffers_mut();
///
///             dst[0].fill(0);
///         }
///
///         io.complete(IoStatus::Success);
///     }
/// }
///
/// /// Implements the NullRs block device.
/// #[derive(Default)]
/// struct NullRsCtx;
///
/// unsafe impl Send for NullRsCtx {}
/// unsafe impl Sync for NullRsCtx {}
///
/// #[async_trait]
/// impl BDevOps for NullRsCtx {
///     type IoChannel = NullRsChannel;
///
///     async fn destruct(&mut self) -> Result<(), Errno> {
///         Ok(())
///     }
///
///     fn io_type_supported(&self, io_type: IoType) -> bool {
///         matches!(io_type, IoType::Read | IoType::Write)
///     }
///
/// }
///
/// /// Implements the owned device wrapper governing the NullRs device lifetime.
/// struct NullRs(NonNull<spdk_bdev>);
///
/// impl NullRs {
///     pub fn try_new() -> Result<Device<Self>, Errno> {
///         let mut null = NullRsModule::new_bdev(c_str!("null-rs"), NullRsCtx::default());
///
///         null.bdev.blocklen = 4096;
///         null.bdev.blockcnt = 1;
///
///         null.register()?;
///
///         Ok(Device::new(NullRs(unsafe { NonNull::new_unchecked(null.into_bdev_ptr()) })))
///     }
/// }
///
/// unsafe impl Send for NullRs {}
/// unsafe impl Sync for NullRs {}
///
/// impl From<Owned> for NullRs {
///     fn from(owned: Owned) -> Self {
///         NullRs(unsafe { NonNull::new_unchecked(owned.into_ptr()) })
///     }
/// }
///
/// #[async_trait]
/// impl OwnedOps for NullRs {
///     fn as_ptr(&self) -> *mut spdk_bdev {
///         self.0.as_ptr()
///     }
///
///     async fn destroy(self) -> Result<(), Errno> {
///         Promise::new(move |cx| {
///             unsafe {
///                 spdk_bdev_unregister(
///                     self.as_ptr(),
///                     Some(complete_with_status),
///                     cx);
///             }
///
///             Poll::Pending
///         }).await
///     }
/// }
///
/// /// A program that creates and writes to the NullRs block device.
/// #[spdk::main]
/// async fn main() {
///     let null = NullRs::try_new().unwrap();
///     let desc = null.open(true).await.unwrap();
///     let mut ch = desc.io_channel().unwrap();
///     let layout = null.layout_for_blocks(1).unwrap();
///     let mut buf = dma::Buffer::new_zeroed(layout);
///
///     write!(buf.cursor_mut(), "Hello, World!").unwrap();
///
///     ch.write_at(&buf, 0).await.unwrap();
///
///     drop(ch);
///     drop(desc);
///     null.destroy().await.unwrap();
/// }
/// ```
/// 
/// [`instance()`]: ../spdk/bdev/trait.ModuleInstance.html#tymethod.instance
/// [`new_bdev()`]: ../spdk/bdev/trait.ModuleInstance.html#tymethod.new_bdev
/// [`Module<T>`]: ../spdk/bdev/struct.Module.html
/// [`ModuleInstance`]: ../spdk/bdev/trait.ModuleInstance.html
#[cfg(feature = "bdev-module")]
#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    module::GenerateModule::new().generate(attr, item)
}