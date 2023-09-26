use std::{
    mem::MaybeUninit,
    ops::{
        BitAndAssign,
        BitOrAssign,
        BitXorAssign,
        Not,
    },
};

use spdk_sys::{
    spdk_cpuset,
    
    spdk_cpuset_and,
    spdk_cpuset_copy,
    spdk_cpuset_count,
    spdk_cpuset_equal,
    spdk_cpuset_negate,
    spdk_cpuset_or,
    spdk_cpuset_set_cpu,
    spdk_cpuset_xor,
    spdk_cpuset_zero,
};

pub struct CpuSet(spdk_cpuset);

impl CpuSet {
    /// Create a new empty CPU set.
    pub fn new() -> Self {
        Default::default()
    }

    pub fn as_ptr(&self) -> *const spdk_cpuset {
        &self.0 as *const _ as *const _
    }

    /// Set or clear the CPU state in the set.
    pub fn set_cpu(&mut self, cpu: u32, enable: bool) {
        unsafe {
            spdk_cpuset_set_cpu(&mut self.0 as *mut _ as *mut _, cpu, enable);
        }
    }

    /// Clear all CPUs in the set.
    pub fn clear(&mut self) {
        unsafe {
            spdk_cpuset_zero(&mut self.0 as *mut _ as *mut _);
        }
    }

    /// Returns the number of CPUs in the set.
    pub fn count(&self) -> u32 {
        unsafe {
            spdk_cpuset_count(&self.0 as *const _ as *const _)
        }
    }
}

impl Default for CpuSet {
    fn default() -> Self {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_zero(&mut cpu_set as *mut _ as *mut _);

            Self(cpu_set.assume_init())
        }
    }
}

impl PartialEq for CpuSet {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            spdk_cpuset_equal(&self.0 as *const _ as *const _, &other.0 as *const _ as *const _)
        }
    }
}

impl Eq for CpuSet {}

impl Clone for CpuSet {
    fn clone(&self) -> Self {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_copy(&mut cpu_set as *mut _ as *mut _, &self.0 as *const _ as *const _);

            Self(cpu_set.assume_init())
        }
    }
}

impl Copy for CpuSet {}

impl BitAndAssign for CpuSet {
    fn bitand_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_and(&mut self.0 as *mut _ as *mut _, &rhs.0 as *const _ as *const _);
        }
    }
}

impl BitOrAssign for CpuSet {
    fn bitor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_or(&mut self.0 as *mut _ as *mut _, &rhs.0 as *const _ as *const _);
        }
    }
}

impl BitXorAssign for CpuSet {
    fn bitxor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_xor(&mut self.0 as *mut _ as *mut _, &rhs.0 as *const _ as *const _);
        }
    }
}

impl Not for CpuSet {
    type Output = Self;

    fn not(self) -> Self::Output {
        unsafe {
            let mut cpu_set = MaybeUninit::<spdk_cpuset>::uninit();

            spdk_cpuset_copy(&mut cpu_set as *mut _ as *mut _, &self.0 as *const _ as *const _);

            spdk_cpuset_negate(&mut cpu_set as *mut _ as *mut _);

            Self(cpu_set.assume_init())
        }
    }
}
