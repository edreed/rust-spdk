#!/usr/bin/env bash

# Install the Rust toolchain if not already installed.
if [[ -z "$(command -v cargo)" ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi

# Install UV if not already installed
if [[ -z "$(command -v uv)" ]]; then
    curl -LsSf https://astral.sh/uv/install.sh | sh
fi

# Clone the SPDK repo at the supported version, install dependencies, build and install it.
SPDK_VERSION="v26.09"
INSTALL_DIR="/usr/local/share/spdk-${SPDK_VERSION}"
SRC_BASEDIR="/usr/local/src"
SPDK_SRC_DIR="${SRC_BASEDIR}/spdk-${SPDK_VERSION}"

mkdir -p "${SRC_BASEDIR}"
cd "${SRC_BASEDIR}" || exit

if [[ -e "${SPDK_SRC_DIR}" ]]; then
    cd "${SPDK_SRC_DIR}" || exit
else
    git clone --branch "${SPDK_VERSION}" --single-branch --depth 1 https://github.com/spdk/spdk.git "${SPDK_SRC_DIR}"
    cd "${SPDK_SRC_DIR}" || exit
    git submodule update --init --recursive --depth 1
fi

./scripts/pkgdep.sh
./configure \
    --prefix="${INSTALL_DIR}" \
    --disable-apps \
    --disable-examples \
    --disable-tests \
    --disable-unit-tests \
    --with-uring \
    --with-vfio-user \
    --without-shared

NPROC=$(nproc 2>/dev/null || echo 1)

make -j $NPROC
make install

VFIO_USER_INSTALL_DIR="${INSTALL_DIR}/libvfio-user"
mkdir -p "${VFIO_USER_INSTALL_DIR}"
cp -R ${SPDK_SRC_DIR}/build/libvfio-user/usr/local/* "${VFIO_USER_INSTALL_DIR}"

make clean
