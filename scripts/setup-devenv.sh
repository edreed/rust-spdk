#!/usr/bin/env bash

set -euox pipefail

export SCRIPT_DIR
export SCRIPT_NAME

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
SCRIPT_NAME="$(basename "$(realpath "$0")" .sh)"

# Install the Rust toolchain if not already installed.
if [[ -z "$(command -v cargo)" ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "${HOME}/.profile"
fi

# Install the toml-cli tool if not already installed.
if [[ -z "$(command -v toml)" ]]; then
    cargo install toml
fi

# The remainder of this script must be run as root.
[ "$(id -u)" -ne 0 ] && exec sudo -E -H env PATH="${PATH}" "$0" "$@"

# Install dependencies and perform setup specific to the current distribution.
source /etc/os-release
OS_SCRIPT="${SCRIPT_DIR}/${SCRIPT_NAME}/${ID}.sh"
if [ ! -f "${OS_SCRIPT}" ]; then
    OS_SCRIPT="${SCRIPT_DIR}/${SCRIPT_NAME}/${ID_LIKE}.sh"
    if [ ! -f "${OS_SCRIPT}" ]; then
        echo "ERROR: The distribution ${PRETTY_NAME:-${NAME}} is not supported."
    fi
fi

source "${OS_SCRIPT}"

# Install dependencies and perform setup common for all supported distributions.
source "${SCRIPT_DIR}/${SCRIPT_NAME}/common.sh"
