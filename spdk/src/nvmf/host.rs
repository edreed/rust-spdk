use std::{
    ffi::CStr,
    ptr::NonNull,
};

use spdk_sys::{
    spdk_nvmf_host,
    
    spdk_nvmf_host_get_nqn,
    spdk_nvmf_subsystem_get_first_host,
    spdk_nvmf_subsystem_get_next_host,
};

use super::Subsystem;

/// Represents a host allowed to connect to a NVMe-oF subsystem.
pub struct Host(NonNull<spdk_nvmf_host>);

unsafe impl Send for Host {}

impl Host {
    /// Returns a host from a raw `spdk_nvmf_host` pointer.
    pub fn from_ptr(ptr: *mut spdk_nvmf_host) -> Self {
        match NonNull::new(ptr) {
            Some(ptr) => Self(ptr),
            None => panic!("host pointer must not be null"),
        }
    }

    /// Returns a pointer to the underlying `spdk_nvmf_host` structure.
    pub fn as_ptr(&self) -> *mut spdk_nvmf_host {
        self.0.as_ptr()
    }

    /// Returns the NQN of the host.
    pub fn nqn(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_nvmf_host_get_nqn(self.as_ptr())) }
    }
}

/// An iterator over the hosts allowed to connect to a NVMe-oF subsystem.
pub struct AllowedHosts<'a> {
    subsys: &'a Subsystem,
    next: *mut spdk_nvmf_host,
}

unsafe impl Send for AllowedHosts<'_> {}

impl <'a> AllowedHosts<'a> {
    /// Creates a new iterator over the hosts allowed to connect to a NVMe-oF
    /// subsystem.
    pub(crate) fn new(subsys: &'a Subsystem) -> Self {
        Self {
            subsys,
            next: unsafe {
                spdk_nvmf_subsystem_get_first_host(subsys.as_ptr())
            },
        }
    }
}

impl Iterator for AllowedHosts<'_> {
    type Item = Host;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.next.is_null() {
                return None;
            }

            let next = self.next;
            self.next = spdk_nvmf_subsystem_get_next_host(
                self.subsys.as_ptr(),
                self.next);

            Some(Host::from_ptr(next))
        }
    }
}
