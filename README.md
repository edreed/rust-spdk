# rust-spdk

Rust programming language support for the Storage Performance Development Kit ([SPDK]).

## Overview

A crate for writing scalable, high performance, asynchronous user-mode storage applications in Rust
using the Storage Performance Development Kit ([SPDK]).

The `rust-spdk` crate provides low- to zero-cost abstractions in Rust built on features and concepts
of the SPDK. It implements an asynchronous runtime using the SPDK’s [Message Passing and
Concurrency] and [Event Framework].

Creating an asynchronous `rust-spdk`-based application is as simple as adding the `spdk::main`
attribute to your `async fn main()` function. The `main` function is run on a special, dedicated
SPDK application thread. This thread is always associated with the main CPU core assigned to the
application.

### Application Example

The following example creates a basic `rust-spdk` application that creates an instance of the
`Malloc` RAM disk block device and then writes to and reads from it.

```rust
use std::{
    ffi::CStr,
    io::{Read, Write},
};

use spdk::{bdev::MallocBuilder, dma, thread};

const BDEV_NAME: &CStr = c"Malloc0";
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const DATA: &str = "Hello, World!";

#[spdk::main]
async fn main() {
    // Create a new Malloc block device.
    let malloc = MallocBuilder::new()
        .with_name(BDEV_NAME)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .await
        .unwrap()
        .into_owned();
    let devname = malloc.name().to_string_lossy().to_string();

    thread::spawn_local(async {
        // Open the underlying block device in a separate asynchronous task to scope the lifetime of
        // the descriptor and I/O channel. These must be dropped before the Malloc block device can
        // be destroyed.
        let desc = malloc.open(true).unwrap();
        let io_chan = desc.io_channel().unwrap();
        let layout = desc.device().layout_for_blocks(1).unwrap();
        let mut buf = dma::Buffer::new_zeroed(layout).unwrap();

        println!("Writing \"{}\" to {}...", DATA, devname);

        write!(buf.cursor_mut(), "{}", DATA).unwrap();

        io_chan.write_at(&desc, &buf, 0).await.unwrap();

        buf.clear();

        io_chan.read_at(&desc, &mut buf, 0).await.unwrap();

        let mut read_data = String::new();

        buf.cursor()
            .take(DATA.len() as u64)
            .read_to_string(&mut read_data)
            .unwrap();

        assert_eq!(read_data.as_str(), DATA);

        println!("Read \"{}\" from {}.", read_data, devname);
    })
    .await;

    // Destroy the Malloc block device.
    malloc.destroy().await.unwrap();
}
```

Additional examples can be found [here][ADEX].

## Custom Block Device (BDev) Modules Using `rust-spdk`

The `rust-spdk` crate supports writing custom block device (BDev) modules in asynchronous Rust. Add
the `bdev-module` feature to the `rust-spdk` dependency in your `Cargo.toml` file to enable this
support.

The first step is to define a `struct` representing the context of your custom BDev. Provide the
custom BDev’s implementation using the `BDevOps` trait. If necessary, you can also provide channel-
and/or I/O-specific contexts by defining a `struct` for each and tying them to your custom BDev
through the `BDevOps::ChannelContext` and `BDevOps::IoContext` associated types, respectively. These
associated types can be assigned the unit (`()`) type if no additional context is needed by your
custom BDev.

Next, define a `struct` representing the global module context for your custom BDev. Provide the
custom BDev’s implementation using the `ModuleOps` trait, assigning your custom BDev `struct` to the
`ModuleOps::BDev` associated type. All of the `ModuleOps` traits methods are provided for you, so
you are not required to provide any implementations for it. However, global module initialization
and finalization can be performed by providing your own implementations of the `ModuleOps::init` and
`ModuleOps::fini` methods, respectively. Virtual BDevs may also provide implementations of the
`ModuleOps::examine_config` and `ModuleOps::examine_disk` methods to examine and claim ownership of,
if necessary, other BDev instances as they are registered with the SPDK BDev subsystem.

Finally, decorate your module `struct` with the `spdk::module` attribute and start using it from
your `rust-spdk` application. The `ModuleOps::new_bdev_builder` returns a builder object that you
use to create a new instance of your custom BDev.

### Custom BDev Example

The following example implements a simple BDev that ignores writes and returns zeroed blocks on
read.

```rust
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
    type BDev<'bdev> = NullRs;
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
```

The [Echo BDev Module] and [PassthruRs Virtual BDev Module] examples demonstrate more complex BDev
implementations.

## Feature Flags

- **`bdev`** — Enables basic block device (BDev) support.
- **`bdev-aio`** — Enables support for the Linux AIO BDev.
- **`bdev-malloc`** — Enables support for the Malloc BDev.
- **`bdev-module`** — Enables support for writing custom BDev modules.
- **`bdev-uring`** — Enables support for the Uring BDev.
- **`json`** — Enables support for JSON serialization and deserialization through the SPDK.
- **`json-rpc`** — Enables writing JSON-RPC functions.
- **`net`** — Enables SPDK-based network socket support.
- **`nvme`** — Enables NVMe.
- **`nvme-vfio-user`** — Enables the `vfio-user` transport for the NVMe-oF target.
- **`nvmf`** — Enables the NVMe-oF target.
- **`scsi`** — Enables SCSI.

## Documentation

Full documentation can be found [here](https://edreed.github.io/doc/rust-spdk/spdk/index.html).

## Usage

Add the following dependency to your `Cargo.toml`:

```toml
[dependencies]
rust-spdk = { git = "https://github.com/edreed/rust-spdk", branch = "main" }
```

You then need to build and install the SPDK. The script `scripts/setup-devenv.sh` handles installing
all of the SPDK's dependencies, builds and installs it into the default locations described in the
following table.

| Description | Environment Override | Default Location                                   |
|-------------|----------------------|----------------------------------------------------|
| Sources     | `SPDK_SRC_DIR`       | `/usr/local/src/spdk-${SPDK_VERSION}`              |
| Libraries   | `SPDK_SHARE_DIR`     | `/usr/local/share/spdk-${SPDK_VERSION}/${PROFILE}` |

`SPDK_VERSION` is the SPDK release version currently supported by `rust-spdk`. Curently, this is
`v26.09`. `PROFILE` is either `release` or `debug`. The `debug` profile is used if debug information
is generated during the build. See [Profiles / Profile settings / debug][DBG] in "The Cargo Book"
for more information on controlling debug information generation.

You can also provide your own copy of the SPDK sources and libraries by specifying paths to them in
the `SPDK_SRC_DIR` and `SPDK_SHARE_DIR` environment variables when building your application. The
following example clones the SPDK into `/spdk/src`, and then builds and installs it to
`/spdk/share`.

```sh
export SPDK_SRC=/spdk/src
export SPDK_INSTALL=/spdk/share

# Clone and install the SPDK
mkdir -p "$SPDK_SRC"
git clone https://github.com/spdk/spdk.git "$SPDK_SRC$"
cd "$SPDK_SRC"
./configure --prefix "$SPDK_INSTALL" --with-uring --with-vfio-user
make
make install

# The libvfio-user directory must be manually copied to the install location
mkdir -p "$SPDK_INSTALL/libvfio-user"
cp -R build/libvfio-user/usr/share/local/* "$SPDK_INSTALL/libvfio-user"
```

Build your Rust project with your installation of the SPDK.

```sh
SPDK_SRC_DIR=/spdk/src SPDK_SHARE_DIR=/spdk/share cargo build
```

## License

Licensed under the [MIT license](LICENSE).

## Contributing

**COMING SOON**

[ADEX]: https://github.com/edreed/rust-spdk/tree/main/spdk/examples
[SPDK]: https://www.spdk.io/doc/about.html
[Echo BDev Module]: https://github.com/edreed/rust-spdk/blob/main/spdk/examples/module_echo.rs
[PassthruRs Virtual BDev Module]:
    https://github.com/edreed/rust-spdk/blob/main/spdk/examples/module_passthru.rs
[Message Passing and Concurrency]: https://spdk.io/doc/concurrency.html
[Event Framework]: https://spdk.io/doc/event.html
[DBG]: https://doc.rust-lang.org/cargo/reference/profiles.html#debug
