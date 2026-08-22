#include "spdk/cpuset.h"
#include "spdk/env.h"
#include "spdk/event.h"
#include "spdk/thread.h"

#if defined(CARGO_FEATURE_BDEV)
#include "spdk/bdev.h"
#include "spdk/bdev_module.h"
#include "spdk/bdev_zone.h"
#endif

#if defined(CARGO_FEATURE_BDEV_MALLOC)
#include "bdev/malloc/bdev_malloc.h"
#endif

#if defined(CARGO_FEATURE_JSON)
#include "spdk/json.h"
#endif

#if defined(CARGO_FEATURE_NET)
#include "spdk/sock.h"
#endif

#if defined(CARGO_FEATURE_NVME)
#include "spdk/nvme.h"
#include "spdk/nvme_spec.h"
#endif

#if defined(CARGO_FEATURE_NVMF)
#include "spdk/nvmf.h"
#include "spdk/nvmf_spec.h"
#include "event/subsystems/nvmf/event_nvmf.h"
#endif

#if defined(CARGO_FEATURE_SCSI)
#include "spdk/scsi.h"
#include "spdk/scsi_spec.h"
#endif

#if defined(CARGO_FEATURE_BDEV)
/**
 * Return the status of a BDev I/O.
 *
 * @param bdev_io A pointer to an `spdk_bdev_io` structure.
 *
 * @return The status of the BDev I/O.
 */
enum spdk_bdev_io_status spdk_bdev_io_get_status(struct spdk_bdev_io *bdev_io);
#endif
