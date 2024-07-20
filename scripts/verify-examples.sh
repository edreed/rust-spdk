#!/usr/bin/env bash

set -euo pipefail

print_banner() {
    echo "=============================================================================="
    echo "$*"
    echo "=============================================================================="
}

run_example() {
    local EXAMPLE="$1"
    shift

    print_banner "Running ${EXAMPLE} example"
    cargo run --example "${EXAMPLE}" "$@"
    print_banner "Example ${EXAMPLE} completed successfully"
    echo
}

COMMON_ARGS=("--iova-mode=va" "--huge-dir=/mnt/hugepages")

run_example bdev_hello_world --features="bdev-malloc" -- "${COMMON_ARGS[@]}"
run_example cli -- "${COMMON_ARGS[@]}" --block-size=4096 --create-new
run_example interval -- "${COMMON_ARGS[@]}"
run_example module_echo --features="bdev-module" -- "${COMMON_ARGS[@]}" --lcores="(0,1)"
run_example module_null --features="bdev-module" -- "${COMMON_ARGS[@]}"
run_example module_passthru --features="bdev-malloc,bdev-module" -- "${COMMON_ARGS[@]}"
run_example reactor -- "${COMMON_ARGS[@]}"
run_example reactor -- "${COMMON_ARGS[@]}"
run_example sleep -- "${COMMON_ARGS[@]}"
run_example thread -- "${COMMON_ARGS[@]}"
