#!/usr/bin/env bash

set -euo pipefail

print_banner() {
    echo "=============================================================================="
    echo "$*"
    echo "=============================================================================="
}

print_banner_minor() {
    echo "------------------------------------------------------------------------------"
    echo "$*"
    echo "------------------------------------------------------------------------------"
}

run_example() {
    local EXAMPLE="$1"
    shift

    print_banner "Running ${EXAMPLE} example"
    cargo run --example "${EXAMPLE}" "$@"
    print_banner "Example ${EXAMPLE} completed successfully"
    echo
}

verify_jsonrpc() {
    local JSONRPC="${GITROOT}/target/debug/examples/jsonrpc"

    print_banner_minor 'Building "jsonrpc" application'
    cargo build --example jsonrpc --features=json-rpc

    print_banner_minor 'Starting "jsonrpc" application'
    "$JSONRPC" "${COMMON_ARGS[@]}" &

    sleep 1

    print_banner_minor 'Calling "hello_world" RPC function'
    "$SPDKRPC" --plugin jsonrpc_plugin hello_world

    print_banner_minor 'Calling "print_message" RPC function'
    "$SPDKRPC" --plugin jsonrpc_plugin print_message "Hello, There!"

    print_banner_minor 'Calling "add_two" RPC function'
    "$SPDKRPC" --plugin jsonrpc_plugin add_two 41 1

    print_banner_minor 'Calling "guess_number" RPC function'
    "$SPDKRPC" --plugin jsonrpc_plugin guess_number 5

    print_banner_minor 'Stopping "jsonrpc" application'
    "$SPDKRPC" --plugin jsonrpc_plugin spdk_kill_instance SIGINT
}

GITROOT="$(git rev-parse --show-toplevel)"
SPDKRPC="${GITROOT}/scripts/spdk_rpc.sh"

COMMON_ARGS=("--iova-mode=va" "--huge-dir=/mnt/hugepages")

run_example bdev_aio --features="bdev-aio" -- "${COMMON_ARGS[@]}"
run_example bdev_all --features="bdev-malloc" -- "${COMMON_ARGS[@]}"
run_example bdev_hello_world --features="bdev-malloc" -- "${COMMON_ARGS[@]}"
run_example bdev_uring --features="bdev-uring" -- "${COMMON_ARGS[@]}"
run_example cli -- "${COMMON_ARGS[@]}" --block-size=4096 --create-new
run_example env -- "${COMMON_ARGS[@]}" --lcores="(0-3)"
run_example hello_world -- "${COMMON_ARGS[@]}"
run_example interval -- "${COMMON_ARGS[@]}"
run_example module_echo --features="bdev-module" -- "${COMMON_ARGS[@]}" --lcores="(0,1)"
run_example module_null --features="bdev-module" -- "${COMMON_ARGS[@]}"
run_example module_passthru --features="bdev-malloc,bdev-module" -- "${COMMON_ARGS[@]}"
run_example net_resolve --features="net" -- "${COMMON_ARGS[@]}"
run_example reactor -- "${COMMON_ARGS[@]}"
run_example runtime -- "${COMMON_ARGS[@]}"
run_example sleep -- "${COMMON_ARGS[@]}"
run_example thread -- "${COMMON_ARGS[@]}"

verify_jsonrpc
