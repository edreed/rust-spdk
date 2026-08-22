#include "spdk/bdev_module.h"

#if defined(CARGO_FEATURE_BDEV)
enum spdk_bdev_io_status spdk_bdev_io_get_status(struct spdk_bdev_io *bdev_io) {
    return bdev_io->internal.status;
}
#endif
