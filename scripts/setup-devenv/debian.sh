#!/usr/bin/env bash

# Install tools and libraries needed to build the SPDK.
add-apt-repository -y ppa:git-core/ppa
apt-get update
apt-get install -y \
    build-essential \
    clang \
    curl \
    git \
    libaio-dev \
    libjson-c-dev \
    libcmocka-dev \
    meson \
    pkg-config \
    python3 \
    python3-jinja2
