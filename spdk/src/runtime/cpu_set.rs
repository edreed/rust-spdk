use std::{
    mem::MaybeUninit,
    ops::{BitAndAssign, BitOrAssign, BitXorAssign, Not},
};

use spdk_sys::{
    spdk_cpuset, spdk_cpuset_and, spdk_cpuset_copy, spdk_cpuset_count, spdk_cpuset_equal,
    spdk_cpuset_negate, spdk_cpuset_or, spdk_cpuset_set_cpu, spdk_cpuset_xor, spdk_cpuset_zero,
};

pub struct CpuSet(spdk_cpuset);

impl CpuSet {
    /// Create a new empty CPU set.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns a pointer to the underlying `spdk_cpuset` structure.
    pub(crate) fn as_ptr(&self) -> *const spdk_cpuset {
        &self.0 as *const _
    }

    /// Returns a  pointer to the mutable underlying `spdk_cpuset` structure.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut spdk_cpuset {
        &self.0 as *const _ as *mut _
    }

    /// Set or clear the CPU state in the set.
    pub fn set_cpu(&mut self, cpu: u32, enable: bool) {
        unsafe {
            spdk_cpuset_set_cpu(self.as_mut_ptr(), cpu, enable);
        }
    }

    /// Clear all CPUs in the set.
    pub fn clear(&mut self) {
        unsafe {
            spdk_cpuset_zero(self.as_mut_ptr());
        }
    }

    /// Returns the number of CPUs in the set.
    pub fn count(&self) -> u32 {
        unsafe { spdk_cpuset_count(self.as_ptr()) }
    }
}

impl Default for CpuSet {
    fn default() -> Self {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_zero(cpu_set.as_mut_ptr());

            Self(cpu_set.assume_init())
        }
    }
}

impl PartialEq for CpuSet {
    fn eq(&self, other: &Self) -> bool {
        unsafe { spdk_cpuset_equal(self.as_ptr(), other.as_ptr()) }
    }
}

impl Eq for CpuSet {}

impl Clone for CpuSet {
    fn clone(&self) -> Self {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_copy(cpu_set.as_mut_ptr(), self.as_ptr());

            Self(cpu_set.assume_init())
        }
    }
}

impl Copy for CpuSet {}

impl BitAndAssign for CpuSet {
    fn bitand_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_and(self.as_mut_ptr(), rhs.as_ptr());
        }
    }
}

impl BitOrAssign for CpuSet {
    fn bitor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_or(self.as_mut_ptr(), rhs.as_ptr());
        }
    }
}

impl BitXorAssign for CpuSet {
    fn bitxor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_xor(self.as_mut_ptr(), rhs.as_ptr());
        }
    }
}

impl Not for CpuSet {
    type Output = Self;

    fn not(self) -> Self::Output {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_copy(cpu_set.as_mut_ptr(), self.as_ptr());
            spdk_cpuset_negate(cpu_set.as_mut_ptr());

            Self(cpu_set.assume_init())
        }
    }
}

impl From<spdk_cpuset> for CpuSet {
    fn from(cpu_set: spdk_cpuset) -> Self {
        Self(cpu_set)
    }
}
