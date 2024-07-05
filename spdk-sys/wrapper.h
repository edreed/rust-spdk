#include "spdk/bdev.h"
#include "spdk/bdev_zone.h"
#include "spdk/cpuset.h"
#include "spdk/env.h"
#include "spdk/event.h"
#include "spdk/thread.h"

#if defined(CARGO_FEATURE_BDEV_MODULE)
#include "spdk/bdev_module.h"
#endif

#if defined(CARGO_FEATURE_BDEV_MALLOC)
#include "bdev/malloc/bdev_malloc.h"
#endif

#if defined(CARGO_FEATURE_NVMF)
#include "spdk/nvme.h"
#include "spdk/nvmf.h"
#include "spdk/nvmf_spec.h"
#include "event/subsystems/nvmf/event_nvmf.h"
#endif

/**
 * Return a pointer to the mempool owning this object.
 *
 * @param obj
 *   An object that is owned by a pool. If this is not the case,
 *   the behavior is undefined.
 * @return
 *   A pointer to the mempool structure.
 */
struct spdk_mempool *spdk_mempool_from_obj(void *obj);
