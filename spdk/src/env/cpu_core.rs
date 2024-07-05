use std::mem::transmute;

use spdk_sys::{
    spdk_env_core_get_smt_cpuset, spdk_env_get_core_count, spdk_env_get_current_core,
    spdk_env_get_first_core, spdk_env_get_main_core, spdk_env_get_next_core,
};

use crate::env::NumaNode;

use super::CpuSet;

const SPDK_ENV_LCORE_ID_ANY: u32 = u32::MAX;

/// Represents a dedicated CPU core for this environment.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct CpuCore(u32);

unsafe impl Send for CpuCore {}
unsafe impl Sync for CpuCore {}

impl CpuCore {
    /// Tries to return the current CPU core.
    ///
    /// # Returns
    ///
    /// If the current system thread is a dedicated SPDK CPU core, this function
    /// returns `Some(c)` where `c` is the current CPU core. Otherwise, this
    /// function returns `None`.
    pub fn try_current() -> Option<Self> {
        let core_id = unsafe { spdk_env_get_current_core() };

        if core_id != SPDK_ENV_LCORE_ID_ANY {
            return Some(unsafe { transmute::<u32, Self>(core_id) });
        }

        None
    }

    /// Returns the current CPU core.
    ///
    /// # Panics
    ///
    /// This function panics if the current system thread is not a on dedicated
    /// SPDK core.
    pub fn current() -> Self {
        Self::try_current().expect("must be called on a dedicated CPU core")
    }

    /// Returns the main core for this environment.
    pub fn main() -> Self {
        Self(unsafe { spdk_env_get_main_core() })
    }

    /// Returns the ID of this CPU core.
    pub fn id(&self) -> u32 {
        self.0
    }

    /// Returns the NUMA node of this CPU core.
    pub fn numa_node(&self) -> NumaNode {
        NumaNode::of_core(self)
    }

    /// Returns a [`CpuSet`] with each [SMT] sibling core's bit set to `true`.
    ///
    /// If the specified core has no siblings, then only the specified core's bit will be set. If
    /// the core has SMT siblings, then all siblings, including the specified core, will be set.
    ///
    /// NOTE: The bits will be set for all SMY siblings regardless of whether they are part of the
    /// application's core mask.
    ///
    /// A `CpuSet` is only returned if SMT detection is enabled for the underlying platform.
    /// Otherwise, this method return `None`.
    ///
    /// [SMT]: https://en.wikipedia.org/wiki/Simultaneous_multithreading
    pub fn siblings(&self) -> Option<CpuSet> {
        let mut siblings = CpuSet::empty();

        if unsafe { spdk_env_core_get_smt_cpuset(siblings.as_mut_ptr(), self.0) } {
            return Some(siblings);
        }

        None
    }

    /// Returns the first dedicated CPU core for this application.
    fn first() -> Option<CpuCore> {
        let core_id = unsafe { spdk_env_get_first_core() };

        if core_id != u32::MAX {
            return Some(unsafe { transmute::<u32, Self>(core_id) });
        }

        None
    }

    /// Returns the next dedicated CPU core for this application, if there is one.
    fn next(&self) -> Option<CpuCore> {
        let next_id = unsafe { spdk_env_get_next_core(self.0) };

        if next_id != u32::MAX {
            return Some(unsafe { transmute::<u32, Self>(next_id) });
        }

        None
    }
}

impl From<CpuCore> for CpuSet {
    fn from(cpu_core: CpuCore) -> Self {
        let mut cpu_set = CpuSet::empty();

        cpu_set.set_cpu(cpu_core.0, true);

        cpu_set
    }
}

/// An iterator over the dedicated CPU cores for this environment.
pub struct CpuCores {
    next: Option<CpuCore>,
}

unsafe impl Send for CpuCores {}

impl Iterator for CpuCores {
    type Item = CpuCore;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.next {
            self.next = current.next();

            return Some(current);
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let core_count = unsafe { spdk_env_get_core_count() } as usize;

        (core_count, Some(core_count))
    }
}

/// Returns an iterator over the dedicated CPU cores for this environment.
pub fn cpu_cores() -> CpuCores {
    CpuCores {
        next: CpuCore::first(),
    }
}
