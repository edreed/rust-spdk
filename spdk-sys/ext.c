#if defined(CARGO_FEATURE_BDEV)
#include "spdk/bdev_module.h"

enum spdk_bdev_io_status spdk_bdev_io_get_status(struct spdk_bdev_io *bdev_io) {
    return bdev_io->internal.status;
}
#endif

#if defined(CARGO_FEATURE_NET)
#include "spdk_internal/sock_module.h"

void* spdk_sock_get_user_ctx(struct spdk_sock *sock) {
    return sock->cb_arg;
}
#endif
