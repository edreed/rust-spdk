#include "wrapper.h"

#if defined(CARGO_FEATURE_BDEV)
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

#include <rte_lcore.h>
#include <rte_memory.h>
#include <rte_mempool.h>

void spdk_mempool_opts_init(struct spdk_mempool_opts *opts)
{
    memset(opts, 0, sizeof(*opts));
    opts->opts_size = sizeof(*opts);
}

static void mempool_init_cb(struct rte_mempool *mp, void* ctx)
{
    struct spdk_mempool_opts *opts = ctx;

    if (opts->priv_init) {
        (*opts->priv_init)((struct spdk_mempool *)mp, opts->priv_init_arg, rte_mempool_get_priv(mp));
    }
}

struct spdk_mempool *spdk_mempool_create_ext(const char* name, size_t count, size_t ele_size,
    size_t cache_size, int numa_id, struct spdk_mempool_opts *opts)
{
    struct spdk_mempool_opts default_opts;
    struct rte_mempool *mp;
    size_t tmp;

    if (opts == NULL) {
        spdk_mempool_opts_init(&default_opts);
        opts = &default_opts;
    }

    if (numa_id == SPDK_ENV_NUMA_ID_ANY) {
        numa_id = SOCKET_ID_ANY;
    }

    /* No more than half of all elements can be in cache */
    tmp = (count / 2) / rte_lcore_count();
    if (cache_size > tmp) {
        cache_size = tmp;
    }

    if (cache_size > RTE_MEMPOOL_CACHE_MAX_SIZE) {
        cache_size = RTE_MEMPOOL_CACHE_MAX_SIZE;
    }

    mp = rte_mempool_create(name, count, ele_size, cache_size,
                0, mempool_init_cb, opts, (rte_mempool_obj_cb_t *)opts->obj_init, opts->obj_init_arg,
                numa_id, 0);

    return (struct spdk_mempool *)mp;
}

void *spdk_mempool_get_priv(struct spdk_mempool *mp)
{
    return rte_mempool_get_priv((struct rte_mempool *)mp);
}

struct spdk_mempool *spdk_mempool_from_obj(void *obj)
{
    return (struct spdk_mempool *)rte_mempool_from_obj(obj);
}
