#!/usr/bin/env bash

set -euo pipefail

print_banner() {
    echo "=============================================================================="
    echo "$*"
    echo "=============================================================================="
}

build_example() {
    local EXAMPLE="$1"
    shift

    print_banner "Building ${EXAMPLE} example"
    cargo build --example "${EXAMPLE}" "$@"
    print_banner "Example ${EXAMPLE} built successfully"
    echo
}

build_example bdev_hello_world --features="bdev-malloc"
build_example cli
build_example devices --features="bdev-malloc"
build_example interval
build_example module_echo --features="bdev-module"
build_example module_null --features="bdev-module"
build_example module_passthru --features="bdev-malloc,bdev-module"
build_example net_group --features="net"
build_example net_hello_world --features="net"
build_example net_resolve --features="net"
build_example nvmf --features="bdev-malloc,nvmf"
build_example reactor
build_example runtime
build_example sleep
build_example thread
