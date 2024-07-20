#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(dirname "$(realpath "$0")")"

# This script must be run as root.
[ "$(id -u)" -ne 0 ] && exec sudo "$0" "$@"

# Install dependencies specific to the current distribution.
source /etc/os-release
OS_SCRIPT="${SCRIPT_DIR}/prereq/${ID}.sh"
if [ ! -f "${OS_SCRIPT}" ]; then
    OS_SCRIPT="${SCRIPT_DIR}/prereq/${ID_LIKE}.sh"
    if [ ! -f "${OS_SCRIPT}" ]; then
        echo "ERROR: The distribution ${PRETTY_NAME:-${NAME}} is not supported."
    fi
fi