use std::{
    ffi::CStr,
    net::IpAddr,
    time::Duration,
};

use byte_strings::c_str;
use spdk::{
    bdev::malloc,
    cli::Parser,
    nvme::TransportId,
    nvmf::{
        self,

        SubsystemType,
        TransportType,
    },
    runtime::Runtime,
    time::interval,
};
use ternary_rs::if_else;

const BDEV_NAME: &CStr = c_str!("Malloc0");
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const NQN: &CStr = c_str!("nqn.2016-06.io.spdk:cnode1");

#[derive(Parser)]
struct Args {
    /// The IP address to listen on.
    #[spdk_arg(short = 'l', value_name = "IPADDR", default = "127.0.0.1".parse().unwrap())]
    listen_addr: IpAddr,

    /// The port to listen on.
    #[spdk_arg(short = 'P', value_name = "PORT", default = 4420)]
    listen_port: u16,
}

#[spdk::main(cli_args = Args::parse())]
async fn main() {
    let mut target = nvmf::targets().nth(0).unwrap();

    let transport = nvmf::TransportBuilder::new(TransportType::TCP)
        .unwrap()
        .build()
        .await
        .unwrap();

    target.add_transport(transport).await.unwrap();

    let args = Args::get();

    let transport_id = format!(
            "trtype=TCP adrfam={} traddr={} trsvcid={} subnqn={}",
            if_else!(args.listen_addr.is_ipv4(), "IPv4", "IPv6"),
            args.listen_addr,
            args.listen_port,
            NQN.to_string_lossy().to_string())
        .parse::<TransportId>()
        .unwrap();

    target.listen(&transport_id).unwrap();

    let subsys = target.add_subsystem(NQN, SubsystemType::NVMe, 1).unwrap();

    let malloc = malloc::Builder::new()
        .with_name(BDEV_NAME)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .unwrap();

    let malloc_ns = subsys.add_namespace(malloc.name()).unwrap();

    subsys.allow_any_host(true);
    subsys.add_listener(&transport_id).await.unwrap();
    subsys.start().await.unwrap();

    let mut timer = interval(Duration::from_millis(50));

    while !Runtime::is_shutting_down() {
        timer.tick().await;
    }

    target.stop_subsystems().await.unwrap();
    subsys.remove_namespace(malloc_ns.id()).unwrap();
    malloc.destroy().await.unwrap();
    subsys.remove_listener(&transport_id).unwrap();
    target.remove_subsystem(subsys).await.unwrap();
}
