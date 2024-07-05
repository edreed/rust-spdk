#include "spdk/cpuset.h"
#include "spdk/env.h"
#include "spdk/event.h"
#include "spdk/thread.h"
#include "spdk/uuid.h"

#if defined(CARGO_FEATURE_BDEV)
#include "spdk/bdev.h"
#include "spdk/bdev_module.h"
#include "spdk/bdev_zone.h"
#include "spdk/dif.h"
#endif

#if defined(CARGO_FEATURE_BDEV_AIO)
#include "bdev/aio/bdev_aio.h"
#endif

#if defined(CARGO_FEATURE_BDEV_MALLOC)
#include "bdev/malloc/bdev_malloc.h"
#endif

#if defined(CARGO_FEATURE_BDEV_URING)
#include "bdev/uring/bdev_uring.h"
#endif

#if defined(CARGO_FEATURE_JSON)
#include "spdk/json.h"
#endif

#if defined(CARGO_FEATURE_JSON_RPC)
#include "spdk/rpc.h"
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

#if defined(CARGO_FEATURE_NET)
/**
 * Return the user context associated with a socket.
 *
 * @param sock A pointer to an `spdk_sock` structure.
 *
 * @return The user context associated with the socket.
 */
void* spdk_sock_get_user_ctx(struct spdk_sock *sock);
#endif

#include <rte_mempool.h>

/**
 * A callback function to initialize mempool private data.
 *
 * @param mp
 */
typedef void (spdk_mempool_priv_cb_t)(struct spdk_mempool *mp, void *ctx, void* data);


/** Alignment of elements inside a mempool. */
#define SPDK_MEMPOOL_ALIGN RTE_MEMPOOL_ALIGN

struct spdk_mempool_opts
{
    /** The size, in bytes, of this structure. */
    size_t opts_size;

    /**
     * The size of private data, in bytes, to be allocated for this mempool.
     *
     * Private data can be accessed via the `spdk_mempool_priv_get` function.
     */
    size_t priv_size;

    /** An optional user-provided private data initialization callback function. */
    spdk_mempool_priv_cb_t *priv_init;

    /**
     * An optional pointer to a user-defined context passed to the private data initialization
     * callback function.
     */
    void *priv_init_arg;

    /** An optional user-provided object initialization callback function. */
    spdk_mempool_obj_cb_t *obj_init;

    /**
     * An optional pointer to user-defined context passed to the object initialization callback
     * function.
     */
    void *obj_init_arg;
};

/**
 * Initializes an `spdk_mempool_opts` structure.
 *
 * @param opts
 *   A pointer to the `spdk_mempool_opts` structure to initialize.
 */
void spdk_mempool_opts_init(struct spdk_mempool_opts *opts);

/**
 * Creates a thread-safe memory pool with options.
 *
 * @param name
 *   The name of the memory pool.
 * @param count
 *   The count of elements in the memory pool.
 * @param ele_size
 *   The size of elements, in bytes.
 * @param cache_size
 *   The number of element cached in per-core caches. Use `SPDK_MEMPOOL_DEFAULT_CACHE_SIZE` for a
 *   reasonable default, or `0` for no per-core cache.
 * @param numa_id
 *   The NUMA node ID to allocate memory from, or SPDK_ENV_NUMA_ID_ANY for any NUMA node.
 * @param opts
 *   An optional pointer to an `spdk_mempool_opts` structure with additional options. If NULL,
 *   suitable default options will be applied (e.g. no private data or per-object initialization).
 * @return struct spdk_mempool*
 */
struct spdk_mempool *spdk_mempool_create_ext(const char* name, size_t count, size_t ele_size,
    size_t cache_size, int numa_id, struct spdk_mempool_opts *opts);

/**
 * Returns a pointer to the private data in the mempool.
 *
 * @param mp
 *   A pointer to the mempool structure.
 * @return
 *   A pointer to the private data.
 */
void *spdk_mempool_get_priv(struct spdk_mempool *mp);

/**
 * Returns a pointer to the mempool owning this object.
 *
 * @param obj
 *   An object that is owned by a pool. If this is not the case,
 *   the behavior is undefined.
 * @return
 *   A pointer to the mempool structure.
 */
struct spdk_mempool *spdk_mempool_from_obj(void *obj);
