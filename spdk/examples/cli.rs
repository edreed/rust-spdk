use std::path::PathBuf;

use spdk::{self, Uuid, cli::Parser};

#[derive(Debug, Parser)]
struct Args {
    /// Path to the block device to use.
    #[spdk_arg(short = 'D', value_name = "DEVICE")]
    device_path: PathBuf,

    /// The block size of the device in bytes.
    #[spdk_arg(default = 512, value_name = "SIZE")]
    block_size: u32,

    /// If specified, creates a new block device.
    create_new: bool,

    /// The UUID of the block device. If not specified, a new UUID will be generated.
    #[spdk_arg(default = Uuid::new(), value_name = "UUID")]
    uuid: Uuid,
}

#[spdk::main(cli_args = Args::parse())]
async fn main() {
    let args = Args::get();

    println!("{:#?}", args);
}
