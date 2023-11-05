use spdk_sys::{
    spdk_nvmf_ns,
    
    spdk_nvmf_ns_get_bdev,
    spdk_nvmf_ns_get_id,
    spdk_nvmf_subsystem_get_first_ns,
    spdk_nvmf_subsystem_get_next_ns,
};

use crate::{
    bdev::Any,
    block::{self},
};

use super::Subsystem;

/// Represents a namespace in a NVMe-oF subsystem.
pub struct Namespace(pub(crate) *mut spdk_nvmf_ns);

impl Namespace {
    /// Returns the ID of the namespace.
    pub fn id(&self) -> u32 {
        unsafe { spdk_nvmf_ns_get_id(self.0) }
    }

    /// Returns the block device backing the namespace.
    pub fn device(&self) -> block::Device<Any> {
        let bdev = unsafe { spdk_nvmf_ns_get_bdev(self.0) };

        assert!(!bdev.is_null());

        block::Device::from_ptr(bdev)
    }
}

/// An iterator over the namespaces in a NVMe-oF subsystem.
pub struct Namespaces<'a> {
    subsys: &'a Subsystem,
    next: *mut spdk_nvmf_ns,
}

unsafe impl Send for Namespaces<'_> {}

impl <'a> Namespaces<'a> {
    /// Creates a new iterator over the namespaces in a NVMe-oF subsystem.
    pub(crate) fn new(subsys: &'a Subsystem) -> Self {
        Self {
            subsys,
            next: unsafe {
                spdk_nvmf_subsystem_get_first_ns(subsys.as_ptr())
            },
        }
    }
}

impl Iterator for Namespaces<'_> {
    type Item = Namespace;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.next.is_null() {
                return None;
            }

            let namespace = self.next;
            self.next = spdk_nvmf_subsystem_get_next_ns(
                self.subsys.as_ptr(),
                self.next);

            Some(Namespace(namespace))
        }
    }
}