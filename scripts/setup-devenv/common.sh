#!/usr/bin/env bash

install_spdk_profile() {
    local SPDK_SRC="$1"
    local PROFILE="$2"
    local INSTALL_DIR="$3/${PROFILE}"

    local CONFIG_OPTS=(\
        --prefix="${INSTALL_DIR}" \
        --disable-apps \
        --disable-examples \
        --disable-tests \
        --disable-unit-tests \
        --with-uring \
        --with-vfio-user \
        --without-shared \
        )

    if [[ "${PROFILE}" == "debug" ]]; then
        CONFIG_OPTS+=("--enable-debug")
    fi

    cd "${SPDK_SRC}" || exit
    ./configure "${CONFIG_OPTS[@]}"

    NPROC=$(nproc 2>/dev/null || echo 1)

    make -j $NPROC
    make install

    VFIO_USER_INSTALL_DIR="${INSTALL_DIR}/libvfio-user"
    mkdir -p "${VFIO_USER_INSTALL_DIR}"
    cp -R ${SPDK_SRC_DIR}/build/libvfio-user/usr/local/* "${VFIO_USER_INSTALL_DIR}"

    make clean
}

GITROOT="$(git rev-parse --show-toplevel)"

# Clone the SPDK repo at the supported version, install dependencies, build and install it.
SPDK_REPO="$(toml get --raw "${GITROOT}/spdk-sys/config.toml" spdk.repository)"
SPDK_VERSION="$(toml get --raw "${GITROOT}/spdk-sys/config.toml" spdk.version)"

INSTALL_DIR="/usr/local/share/spdk-${SPDK_VERSION}"
SRC_BASEDIR="/usr/local/src"
SPDK_SRC_DIR="${SRC_BASEDIR}/spdk-${SPDK_VERSION}"

mkdir -p "${SRC_BASEDIR}"
cd "${SRC_BASEDIR}" || exit

if [[ -e "${SPDK_SRC_DIR}" ]]; then
    rm -Rf "${SPDK_SRC_DIR}"
fi

git clone --branch "${SPDK_VERSION}" --single-branch --depth 1 "${SPDK_REPO}" "${SPDK_SRC_DIR}"
cd "${SPDK_SRC_DIR}" || exit
git submodule update --init --recursive --depth 1

./scripts/pkgdep.sh --uring

install_spdk_profile "${SPDK_SRC_DIR}" "debug" "${INSTALL_DIR}"
install_spdk_profile "${SPDK_SRC_DIR}" "release" "${INSTALL_DIR}"
