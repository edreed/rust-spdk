#include "spdk/cpuset.h"
#include "spdk/env.h"
#include "spdk/event.h"
#include "spdk/thread.h"

#if defined(CARGO_FEATURE_BDEV)
#include "spdk/bdev.h"
#include "spdk/bdev_zone.h"
#endif

#if defined(CARGO_FEATURE_BDEV_MODULE)
#include "spdk/bdev_module.h"
#endif

#if defined(CARGO_FEATURE_BDEV_MALLOC)
#include "bdev/malloc/bdev_malloc.h"
#endif

#if defined(CARGO_FEATURE_JSON)
#include "spdk/json.h"
#endif

#if defined(CARGO_FEATURE_NVMF)
#include "spdk/nvme.h"
#include "spdk/nvmf.h"
#include "spdk/nvmf_spec.h"
#include "event/subsystems/nvmf/event_nvmf.h"
#endif
