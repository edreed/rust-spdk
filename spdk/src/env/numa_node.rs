use std::{fmt::Display, mem::transmute};

use spdk_sys::{
    SPDK_ENV_NUMA_ID_ANY, spdk_env_get_first_numa_id, spdk_env_get_next_numa_id,
    spdk_env_get_numa_id,
};

use crate::{
    alloc::DmaAllocator,
    env::{CpuCore, CpuSet, cpu_cores},
};

/// Represents a [NUMA] node.
///
/// [NUMA]: https://en.wikipedia.org/wiki/Non-uniform_memory_access
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct NumaNode(i32);

unsafe impl Send for NumaNode {}
unsafe impl Sync for NumaNode {}

impl NumaNode {
    /// A sentinal value representing any NUMA node.
    #[inline]
    pub const fn any() -> Self {
        unsafe { transmute(SPDK_ENV_NUMA_ID_ANY) }
    }

    /// Returns the [`NumaNode`] of the specified [`CpuCore`].
    pub(crate) fn of_core(core: &CpuCore) -> Self {
        unsafe { transmute(spdk_env_get_numa_id(core.id())) }
    }

    /// Returns the ID of this NUMA node.
    #[inline]
    pub fn id(&self) -> i32 {
        self.0
    }

    /// Returns the [`CpuSet`] of CPU cores in the application's core mask that are associated with
    /// this [`NumaNode`].
    ///
    /// If called on the instance returned by [NumaNode::any], the returned `CpuSet` will contain
    /// all of the cores in the application's core mask.
    pub fn cpuset(&self) -> CpuSet {
        if *self == Self::any() {
            return CpuSet::new();
        }

        let mut cpuset = CpuSet::empty();

        cpu_cores()
            .filter(|core| core.numa_node() == *self)
            .for_each(|core| cpuset.set_cpu(core.id(), true));

        cpuset
    }

    /// Returns a [`DmaAllocator`] instance that provides pinned memory buffers from this NUMA node
    /// that are suitable for use with DMA I/O operations.
    ///
    /// Calling this method on the instance returned by [`NumaNode::any`] yields a `DmaAllocator`
    /// that can provide memory buffers from any NUMA node.
    pub fn dma_allocator(&self) -> DmaAllocator {
        (*self).into()
    }

    /// Returns the first [`NumaNode`].
    fn first() -> Self {
        unsafe { transmute(spdk_env_get_first_numa_id()) }
    }

    /// Returns the next [`NumaNode`] if there are any remaining.
    fn next(&self) -> Option<Self> {
        let next_id = unsafe { spdk_env_get_next_numa_id(self.0) };

        if next_id != i32::MAX {
            return Some(unsafe { transmute::<i32, Self>(next_id) });
        }

        None
    }
}

impl Display for NumaNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 != SPDK_ENV_NUMA_ID_ANY {
            return write!(f, "N{}", self.0);
        }

        write!(f, "ANY")
    }
}

/// An iterator over the [`NumaNode`] instances for this environment.
pub struct NumaNodes {
    next: Option<NumaNode>,
}

unsafe impl Send for NumaNodes {}

impl Iterator for NumaNodes {
    type Item = NumaNode;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.next {
            self.next = current.next();

            return Some(current);
        }

        None
    }
}

/// Returns an iterator over the [`NumaNode`] instances for this environment.
pub fn numa_nodes() -> NumaNodes {
    NumaNodes {
        next: Some(NumaNode::first()),
    }
}
