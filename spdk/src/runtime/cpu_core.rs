use spdk_sys::{
    spdk_env_get_core_count,
    spdk_env_get_current_core,
    spdk_env_get_first_core,
    spdk_env_get_main_core,
    spdk_env_get_next_core,
    spdk_env_get_socket_id,
};

const SPDK_ENV_LCORE_ID_ANY: u32 = u32::MAX;

/// Represents a dedicated CPU core for this runtime.
#[derive(Clone, Copy, PartialEq)]
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
        let core = unsafe { spdk_env_get_current_core() };

        if core != SPDK_ENV_LCORE_ID_ANY {
            Some(Self(core))
        } else {
            None
        }
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

    /// Returns the main core for this runtime.
    pub fn main() -> Self {
        Self(unsafe { spdk_env_get_main_core() })
    }

    /// Returns the ID of this CPU core.
    pub fn id(&self) -> u32 {
        self.0
    }

    /// Returns the ID of the socket this CPU core is on.
    pub fn socket_id(&self) -> u32 {
        unsafe { spdk_env_get_socket_id(self.0) }
    }
}

impl From<CpuCore> for u32 {
    fn from(cpu_core: CpuCore) -> Self {
        cpu_core.0
    }
}

/// An iterator over the dedicated CPU cores for this runtime.
pub struct CpuCores(u32);

unsafe impl Send for CpuCores {}

impl Iterator for CpuCores {
    type Item = CpuCore;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == u32::MAX {
            None
        } else {
            let current = self.0;

            self.0 = unsafe { spdk_env_get_next_core(self.0) };

            Some(CpuCore(current))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let core_count = unsafe { spdk_env_get_core_count() } as usize;

        (core_count, Some(core_count))
    }
}

/// Returns an iterator over the dedicated CPU cores for this runtime.
pub fn cpu_cores() -> CpuCores {
    CpuCores(unsafe { spdk_env_get_first_core() })
}
