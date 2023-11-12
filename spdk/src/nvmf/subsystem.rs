use std::{
    ffi::{
        CStr,
        c_void,
    },
    ptr::{
        NonNull,

        null_mut,
    },
};

use spdk_sys::{
    spdk_nvmf_subsystem,
    spdk_nvmf_subtype,
    
    SPDK_NVMF_SUBTYPE_DISCOVERY,
    SPDK_NVMF_SUBTYPE_NVME,

    spdk_nvmf_subsystem_add_host,
    spdk_nvmf_subsystem_add_listener,
    spdk_nvmf_subsystem_add_ns_ext,
    spdk_nvmf_subsystem_get_allow_any_host,
    spdk_nvmf_subsystem_get_first,
    spdk_nvmf_subsystem_get_mn,
    spdk_nvmf_subsystem_get_next,
    spdk_nvmf_subsystem_get_nqn,
    spdk_nvmf_subsystem_get_ns,
    spdk_nvmf_subsystem_get_sn,
    spdk_nvmf_subsystem_get_type,
    spdk_nvmf_subsystem_host_allowed,
    spdk_nvmf_subsystem_pause,
    spdk_nvmf_subsystem_remove_host,
    spdk_nvmf_subsystem_remove_listener,
    spdk_nvmf_subsystem_remove_ns,
    spdk_nvmf_subsystem_resume,
    spdk_nvmf_subsystem_set_allow_any_host,
    spdk_nvmf_subsystem_set_mn,
    spdk_nvmf_subsystem_set_sn,
    spdk_nvmf_subsystem_start,
    spdk_nvmf_subsystem_stop,
};

use crate::{
    errors::{
        Errno,

        EINVAL,
        ENOENT,
    },
    nvme::TransportId,
    task::{
        Promise,

        complete_with_status,
    },
    to_result,
};

use super::{
    AllowedHosts,
    Namespace,
    Namespaces,
    Target,
};


/// The type of a NVMe-oF subsystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubsystemType {
    /// The subsystem is the Discovery controller.
    Discovery = 1,

    /// The subsystem is a NVMe controller.
    NVMe = 2,
}

impl From<spdk_nvmf_subtype> for SubsystemType {
    fn from(value: spdk_nvmf_subtype) -> Self {
        match value {
            SPDK_NVMF_SUBTYPE_DISCOVERY => Self::Discovery,
            SPDK_NVMF_SUBTYPE_NVME => Self::NVMe,
            _ => unreachable!("invalid subsystem type"),
        }
    }
}

impl From<SubsystemType> for spdk_nvmf_subtype {
    fn from(value: SubsystemType) -> Self {
        match value {
            SubsystemType::Discovery => SPDK_NVMF_SUBTYPE_DISCOVERY,
            SubsystemType::NVMe => SPDK_NVMF_SUBTYPE_NVME,
        }
    }
}

/// Represents a NVMe-oF .
pub struct Subsystem(NonNull<spdk_nvmf_subsystem>);

unsafe impl Send for Subsystem {}

impl Subsystem {
    /// Returns a subsystem from a raw `spdk_nvmf_subsystem`.
    pub fn from_ptr(ptr: *mut spdk_nvmf_subsystem) -> Self {
        match NonNull::new(ptr) {
            Some(subsys) => Self(subsys),
            None => panic!("subsystem pointer must not be null"),
        }
    }

    /// Returns the pointer to the underlying `spdk_nvmf_subsystem` structure.
    pub(crate) fn as_ptr(&self) -> *mut spdk_nvmf_subsystem{
        self.0.as_ptr()
    }

    /// Sets the serial number of the subsystem.
    pub fn set_serial_number(&self, sn: &CStr) -> Result<(), Errno> {
        unsafe {
            if spdk_nvmf_subsystem_set_sn(self.as_ptr(), sn.as_ptr()) < 0 {
                return Err(EINVAL);
            }
        }

        Ok(())
    }

    /// Returns the serial number of the subsystem.
    pub fn serial_number(&self) -> &CStr {
        unsafe {
            CStr::from_ptr(spdk_nvmf_subsystem_get_sn(self.as_ptr()))
        }
    }

    /// Sets the model number of the subsystem.
    pub fn set_model_number(&self, mn: &CStr) -> Result<(), Errno> {
        unsafe {
            if spdk_nvmf_subsystem_set_mn(self.as_ptr(), mn.as_ptr()) < 0 {
                return Err(EINVAL);
            }
        }

        Ok(())
    }

    /// Returns the model number of the subsystem.
    pub fn model_number(&self) -> &CStr {
        unsafe {
            CStr::from_ptr(spdk_nvmf_subsystem_get_mn(self.as_ptr()))
        }
    }

    /// Returns the NQN of the subsystem.
    pub fn nqn(&self) -> &CStr {
        unsafe {
            CStr::from_ptr(spdk_nvmf_subsystem_get_nqn(self.as_ptr()))
        }
    }

    /// Return the type of the subsystem.
    pub fn subtype(&self) -> SubsystemType {
        unsafe {
            spdk_nvmf_subsystem_get_type(self.as_ptr()).into()
        }
    }

    /// Returns whether the subsystems is the Discovery controller.
    pub fn is_discovery(&self) -> bool {
        self.subtype() == SubsystemType::Discovery
    }

    /// Sets whether the subsystem allows any host to connect or only hosts in
    /// the allowed list.
    pub fn allow_any_host(&self, allow: bool) {
        unsafe {
            spdk_nvmf_subsystem_set_allow_any_host(self.as_ptr(), allow);
        }
    }

    /// Returns whether the subsystem allows any host to connect or only hosts
    /// in the allowed list.
    pub fn is_any_host_allowed(&self) -> bool {
        unsafe {
            spdk_nvmf_subsystem_get_allow_any_host(self.as_ptr())
        }
    }

    /// Adds a host NQN to the allowed list.
    pub fn allow_host(&self, host_nqn: &CStr) -> Result<(), Errno> {
        unsafe {
            to_result!(spdk_nvmf_subsystem_add_host(self.as_ptr(), host_nqn.as_ptr(), null_mut()))
        }
    }

    /// Removes a host NQN from the allowed list.
    /// 
    /// If a host with the given NQN is already connected, it will not be
    /// disconnected. However, no new connections from the host will be allowed.
    /// 
    /// # Returns
    /// 
    /// Returns `Ok(true)` if the host was removed from the allowed list, and
    /// `Ok(false)` if the host was not in the allowed list.
    pub fn deny_host(&self, host_nqn: &CStr) -> Result<bool, Errno> {
        let res = unsafe {
            to_result!(spdk_nvmf_subsystem_remove_host(self.as_ptr(), host_nqn.as_ptr()))
        };

        match res {
            Ok(()) => Ok(true),
            Err(e) if e == ENOENT => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Returns whether the given host is allowed to connect to the subsystem.
    pub fn is_host_allowed(&self, host_nqn: &CStr) -> bool {
        unsafe {
            spdk_nvmf_subsystem_host_allowed(self.as_ptr(), host_nqn.as_ptr())
        }
    }

    /// Returns an iterator over the hosts allowed to connect to this subsystem.
    pub fn allowed_hosts(&self) -> AllowedHosts {
        AllowedHosts::new(self)
    }

    /// A callback invoked with the result of a NVMe-oF state change operation.
    unsafe extern "C" fn complete_state_change(
        _subsys: *mut spdk_nvmf_subsystem,
        cx : *mut c_void,
        status: i32
    ) {
        complete_with_status(cx, status);
    }

    /// Starts the subsystem.
    /// 
    /// This method transitions of the subsystem from the Inactive to Active state.
    pub async fn start(&self) -> Result<(), Errno> {
        Promise::new(|cx| {
            unsafe {
                to_result!(spdk_nvmf_subsystem_start(
                    self.as_ptr(),
                    Some(Self::complete_state_change),
                    cx as *mut c_void
                ))
            }
        }).await
    }

    /// Stops the subsystem.
    /// 
    /// This method transitions of the subsystem from the Active to Inactive state.
    pub async fn stop(&self) -> Result<(), Errno> {
        Promise::new(|cx| {
            unsafe {
                to_result!(spdk_nvmf_subsystem_stop(
                    self.as_ptr(),
                    Some(Self::complete_state_change),
                    cx as *mut c_void
                ))
            }
        }).await
    }

    /// Pauses the subsystem.
    /// 
    /// This method transitions of the subsystem from the Paused to Inactive state.
    /// 
    /// In a paused state, all admin queues are frozen across the whole
    /// subsystem. If a namespace identifier is provided, all commands to that
    /// namespace are quiesced and incoming commands are queued until the
    /// subsystem is resumed. A namespace identifier of 0 indicates that no
    /// namespace is paused while `SPDK_NVME_GLOBAL_NS_TAG` pauses all namespaces.
    pub async fn pause(&self, ns: u32) -> Result<(), Errno> {
        Promise::new(|cx| {
            unsafe {
                to_result!(spdk_nvmf_subsystem_pause(
                    self.as_ptr(),
                    ns,
                    Some(Self::complete_state_change),
                    cx as *mut c_void
                ))
            }
        }).await
    }

    /// Resumes the subsystem.
    /// 
    /// This method transitions of the subsystem from the Inactive to Paused state.
    pub async fn resume(&self) -> Result<(), Errno> {
        Promise::new(|cx| {
            unsafe {
                to_result!(spdk_nvmf_subsystem_resume(
                    self.as_ptr(),
                    Some(Self::complete_state_change),
                    cx as *mut c_void
                ))
            }
        }).await
    }

    /// Adds the given block device as a namespace on the subsystem.
    /// 
    /// The subsystem must be in the Paused or Inactive states to add a
    /// namespace.
    pub fn add_namespace(&self, device_name: &CStr) -> Result<Namespace, Errno> {
        unsafe {
            let nsid = spdk_nvmf_subsystem_add_ns_ext(
                self.as_ptr(),
                device_name.as_ptr(),
                null_mut(),
                0,
                null_mut());

            if nsid == 0 {
                return Err(EINVAL);
            }

            let ns = spdk_nvmf_subsystem_get_ns(self.as_ptr(), nsid);

            return Ok(Namespace::from_ptr(ns));
        }
    }

    /// Removes a namespace from the subsystem.
    /// 
    /// The subsystem must be in the Paused or Inactive states to remove a
    /// namespace.
    pub fn remove_namespace(&self, nsid: u32) -> Result<(), Errno> {
        unsafe {
            to_result!(spdk_nvmf_subsystem_remove_ns(self.as_ptr(), nsid))
        }
    }

    /// Returns an iterator over the namespaces in the subsystem.
    pub fn namespaces(&self) -> Namespaces {
        Namespaces::new(self)
    }

    /// Adds a listener on the specified transport to the subsystem.
    /// 
    /// The subsystem must be in the Paused or Inactive states to add a
    /// listener.
    pub async fn add_listener(&self, transport_id: &TransportId) -> Result<(), Errno> {
        Promise::new(|cx| {
            unsafe {
                spdk_nvmf_subsystem_add_listener(
                    self.as_ptr(),
                    transport_id.as_ptr() as *mut _,
                    Some(complete_with_status),
                    cx as *mut c_void
                );
            }

            Ok(())
        }).await
    }

    /// Removes a listener on the specified transport from the subsystem.
    /// 
    /// # Returns
    /// 
    /// Returns `Ok(true)` if the listener was removed, and `Ok(false)` if no
    /// listener was listening on the given transport.
    pub fn remove_listener(&self, transport_id: &TransportId) -> Result<bool, Errno> {
        let res = unsafe {
            to_result!(spdk_nvmf_subsystem_remove_listener(
                self.as_ptr(),
                transport_id.as_ptr()))
        };

        match res {
            Ok(()) => Ok(true),
            Err(e) if e == ENOENT => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// An iterator over the subsystems on a NVMe-oF target.
pub struct Subsystems(*mut spdk_nvmf_subsystem);

impl Subsystems {
    pub(crate) fn new(target: &Target) -> Self {
        Self(unsafe { spdk_nvmf_subsystem_get_first(target.as_ptr()) })
    }
}

impl Iterator for Subsystems {
    type Item = Subsystem;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            return None;
        }

        let subsystem = Subsystem::from_ptr(self.0);

        unsafe {
            self.0 = spdk_nvmf_subsystem_get_next(subsystem.as_ptr());
        }

        Some(subsystem)
    }
}