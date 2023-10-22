
use std::{
    env,
    ffi::CStr,
    time::Duration,
};

use byte_strings::c_str;
use spdk::{
    bdev::malloc,
    nvme::TransportId,
    nvmf::{
        self,

        SubsystemType,
        TransportType,
    },
    runtime::Runtime,
    time::interval
};

const BDEV_NAME: &CStr = c_str!("Malloc0");
const NUM_BLOCKS: u64 = 32768;
const BLOCK_SIZE: u32 = 512;

const NQN: &CStr = c_str!("nqn.2016-06.io.spdk:cnode1");

#[spdk::main]
async fn main() {
    let mut target = nvmf::targets().nth(0).unwrap();

    let transport = nvmf::TransportBuilder::new(TransportType::TCP)
        .unwrap()
        .build()
        .await
        .unwrap();

    target.add_transport(transport).await.unwrap();

    let listen_addr = env::var("LISTEN_ADDR").expect("LISTEN_ADDR environment variable must be set");

    let transport_id = format!("trtype=TCP adrfam=IPv4 traddr={} trsvcid=4420 subnqn={}", listen_addr, NQN.to_string_lossy().to_string())
        .parse::<TransportId>()
        .unwrap();

    target.listen(&transport_id).unwrap();

    let malloc_subsys = target.add_subsystem(NQN, SubsystemType::NVMe, 1).unwrap();

    let malloc = malloc::Builder::new()
        .with_name(BDEV_NAME)
        .with_num_blocks(NUM_BLOCKS)
        .with_block_size(BLOCK_SIZE)
        .build()
        .unwrap();

    let _malloc_ns = malloc_subsys.add_namespace(malloc.name()).unwrap();

    malloc_subsys.allow_any_host(true);
    malloc_subsys.add_listener(&transport_id).await.unwrap();
    malloc_subsys.start().await.unwrap();

    let mut timer = interval(Duration::from_millis(50));

    while !Runtime::is_shutting_down() {
        timer.tick().await;
    }

    target.stop_subsystems().await.unwrap();
}
