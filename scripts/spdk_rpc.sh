#!/usr/bin/env bash

set -euo pipefail

if [[ -z "$(command -v toml)" ]]; then
    cargo install toml
fi

GITROOT="$(git rev-parse --show-toplevel)"
SPDK_VERSION="$(toml get --raw "${GITROOT}/spdk-sys/config.toml" spdk.version)"
SPDK_RPC="${SPDK_SRC_DIR:-"/usr/local/src/spdk-${SPDK_VERSION}"}/scripts/rpc.py"
EXAMPLES_DIR="${GITROOT}/spdk/examples"

PLUGINS=()

for plugin in "${EXAMPLES_DIR}"/*_plugin.py; do
    PLUGINS+=(--plugin "$(basename "${plugin}" .py)")
done

PYTHONPATH="${PYTHONPATH:-}:${EXAMPLES_DIR}" "${SPDK_RPC}" "${PLUGINS[@]}" "${@}"
