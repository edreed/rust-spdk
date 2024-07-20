#!/usr/bin/env bash

if [[ -z "$(command -v curl)" ]]; then
    apt-get install -y curl
fi

# Install dependencies common for all supported distributions.
"${SCRIPT_DIR}/prereq/common.sh"

apt-get install clang -y
