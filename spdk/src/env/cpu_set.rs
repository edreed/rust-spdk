use std::{
    ffi::CStr,
    fmt::Display,
    mem::transmute,
    ops::{BitAndAssign, BitOrAssign, BitXorAssign, Not},
    ptr::NonNull,
};

use spdk_sys::{
    spdk_cpuset, spdk_cpuset_alloc, spdk_cpuset_and, spdk_cpuset_copy, spdk_cpuset_count,
    spdk_cpuset_equal, spdk_cpuset_fmt, spdk_cpuset_free, spdk_cpuset_negate, spdk_cpuset_or,
    spdk_cpuset_set_cpu, spdk_cpuset_xor, spdk_cpuset_zero, spdk_env_get_cpuset,
};

/// A bit mask of CPU cores.
#[repr(transparent)]
pub struct CpuSet(NonNull<spdk_cpuset>);

impl CpuSet {
    /// Creates a new [`CpuSet`] with the bit for each dedicated CPU core of this application is set
    /// to `true`.
    pub fn new() -> Self {
        NonNull::new(unsafe { spdk_cpuset_alloc() })
            .map(|cpuset| unsafe {
                spdk_env_get_cpuset(cpuset.as_ptr());
                transmute(cpuset)
            })
            .expect("cpuset allocated")
    }

    /// Creates a new empty CPU set.
    pub fn empty() -> Self {
        NonNull::new(unsafe { spdk_cpuset_alloc() })
            .map(|cpuset| unsafe { transmute(cpuset) })
            .expect("cpuset allocated")
    }

    /// Creates a new CPU set with all CPUs enabled.
    pub fn any() -> Self {
        !Self::empty()
    }

    /// Returns a pointer to the underlying `spdk_cpuset` structure.
    pub(crate) fn as_ptr(&self) -> *const spdk_cpuset {
        self.0.as_ptr()
    }

    /// Returns a mutable pointer to the underlying `spdk_cpuset` structure.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut spdk_cpuset {
        self.0.as_ptr()
    }

    /// Set or clear the CPU state in the set.
    pub fn set_cpu(&mut self, cpu: u32, enable: bool) {
        unsafe {
            spdk_cpuset_set_cpu(self.0.as_ptr(), cpu, enable);
        }
    }

    /// Clear all CPUs in the set.
    pub fn clear(&mut self) {
        unsafe {
            spdk_cpuset_zero(self.0.as_ptr());
        }
    }

    /// Returns the number of CPUs in the set.
    pub fn count(&self) -> u32 {
        unsafe { spdk_cpuset_count(self.0.as_ptr()) }
    }
}

impl Drop for CpuSet {
    fn drop(&mut self) {
        unsafe { spdk_cpuset_free(self.0.as_ptr()) };
    }
}

impl Default for CpuSet {
    /// Creates a new [`CpuSet`] with the bit for each dedicated CPU core of this application is set
    /// to `true`.
    fn default() -> Self {
        Self::new()
    }
}

impl Display for CpuSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = unsafe { CStr::from_ptr(spdk_cpuset_fmt(self.0.as_ptr())) };

        write!(f, "0x{}", s.to_string_lossy().as_ref())
    }
}

impl PartialEq for CpuSet {
    fn eq(&self, other: &Self) -> bool {
        unsafe { spdk_cpuset_equal(self.0.as_ptr(), other.0.as_ptr()) }
    }
}

impl Eq for CpuSet {}

impl Clone for CpuSet {
    fn clone(&self) -> Self {
        let clone = CpuSet::empty();

        unsafe {
            spdk_cpuset_copy(clone.0.as_ptr(), self.0.as_ptr());
        }

        clone
    }
}

impl BitAndAssign for CpuSet {
    fn bitand_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_and(self.0.as_ptr(), rhs.0.as_ptr());
        }
    }
}

impl BitOrAssign for CpuSet {
    fn bitor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_or(self.0.as_ptr(), rhs.0.as_ptr());
        }
    }
}

impl BitXorAssign for CpuSet {
    fn bitxor_assign(&mut self, rhs: Self) {
        unsafe {
            spdk_cpuset_xor(self.0.as_ptr(), rhs.0.as_ptr());
        }
    }
}

impl Not for CpuSet {
    type Output = Self;

    fn not(self) -> Self::Output {
        let neg = self.clone();

        unsafe {
            spdk_cpuset_negate(neg.0.as_ptr());
        }

        neg
    }
}
