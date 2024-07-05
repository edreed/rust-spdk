#include "spdk/env.h"

#include <rte_mempool.h>

struct spdk_mempool *spdk_mempool_from_obj(void *obj)
{
    return (struct spdk_mempool *)rte_mempool_from_obj(obj);
}