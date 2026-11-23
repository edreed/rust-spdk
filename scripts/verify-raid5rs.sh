#!/usr/bin/env bash

set -euo pipefail

print_banner() {
    echo "=============================================================================="
    echo "$*"
    echo "=============================================================================="
}

GITROOT="$(git rev-parse --show-toplevel)"
RAID5RS="${GITROOT}/target/debug/examples/module_raid5"
SPDKRPC="${GITROOT}/scripts/spdk_rpc.sh"

print_banner 'Building "module_raid5" application'
cargo build --example module_raid5 --features=bdev-malloc,bdev-module,json-rpc

print_banner 'Starting "module_raid5" application'
"$RAID5RS" --iova-mode=va --huge-dir=/mnt/hugepages --logflag=all &

sleep 1

print_banner 'Creating "malloc0" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_create -b "malloc0" 64 4096

print_banner 'Creating "malloc1" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_create -b "malloc1" 64 4096

print_banner 'Creating "malloc2" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_create -b "malloc2" 64 4096

print_banner 'Creating "Raid5Rs" BDev'

"$SPDKRPC" --plugin module_raid5_plugin  create_raid5rs_bdev "Raid5Rs" -s 64KiB -c malloc0,malloc1,malloc2

sleep 1

print_banner 'Destroying "Raid5Rs" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  destroy_raid5rs_bdev "Raid5Rs"

print_banner 'Destroying "malloc2" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_delete "malloc2"

print_banner 'Destroying "malloc1" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_delete "malloc1"

print_banner 'Destroying "malloc0" BDev'
"$SPDKRPC" --plugin module_raid5_plugin  bdev_malloc_delete "malloc0"

print_banner 'Stopping "module_raid5" application'
"$SPDKRPC" --plugin module_raid5_plugin  spdk_kill_instance SIGINT
