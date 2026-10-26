#!/usr/bin/env bash
GITROOT="$(git rev-parse --show-toplevel)"
SPDK_RPC="${GITROOT}/spdk-sys/spdk/scripts/rpc.py"
PYTHONPATH="${PYTHONPATH}:${GITROOT}/spdk/examples" "${SPDK_RPC}" --plugin jsonrpc_plugin "${@}"
