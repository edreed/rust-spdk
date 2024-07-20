#!/usr/bin/env bash

# Install the required dependencies for the SPDK.
"${SCRIPT_DIR}/spdk-sys/spdk/scripts/pkgdep.sh"

# Install the Rust toolchain if not already installed.
if [[ -z "$(command -v cargo)" ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
