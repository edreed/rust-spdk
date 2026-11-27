#!/usr/bin/env bash
set -eou pipefail

rm -R target/doc
env RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly doc --package spdk --no-deps --all-features
