use std::{
    ffi::CStr,
    fmt::{
        Debug,
        Display
    },
    mem::{
        MaybeUninit,

        size_of_val, self,
    },
    ptr::NonNull,
    time::Duration,
};

use spdk_sys::{
    spdk_nvmf_transport,
    spdk_nvmf_transport_opts,
    spdk_nvme_transport_type,

    SPDK_NVME_TRANSPORT_CUSTOM,
    SPDK_NVME_TRANSPORT_FC,
    SPDK_NVME_TRANSPORT_NAME_CUSTOM,
    SPDK_NVME_TRANSPORT_NAME_FC,
    SPDK_NVME_TRANSPORT_NAME_PCIE,
    SPDK_NVME_TRANSPORT_NAME_RDMA,
    SPDK_NVME_TRANSPORT_NAME_TCP,
    SPDK_NVME_TRANSPORT_NAME_VFIOUSER,
    SPDK_NVME_TRANSPORT_PCIE,
    SPDK_NVME_TRANSPORT_RDMA,
    SPDK_NVME_TRANSPORT_TCP,
    SPDK_NVME_TRANSPORT_VFIOUSER,

    spdk_nvmf_get_transport_name,
    spdk_nvmf_get_transport_type,
    spdk_nvmf_transport_create_async,
    spdk_nvmf_transport_get_first,
    spdk_nvmf_transport_get_next,
    spdk_nvmf_transport_opts_init,
    spdk_nvmf_transport_destroy,
};

use crate::{
    errors::{
        Errno,

        EINVAL,
        ENODEV,
        ENOMEM,
        EPERM,
    },
    task::{
        Promise,

        complete_with_object,
        complete_with_ok,
    },
    thread,
    to_poll_pending_on_ok,
};

use super::Target;

/// The type of NVMe-oF transport.
/// 
/// # Notes
/// 
/// These are mapped directly to the NVMe over Fabrics TRTYPE values, except for
/// PCIe, which is a special case since NVMe over Fabrics does not define a
/// TRTYPE for local PCIe.
/// 
/// Transports supported by SPDK but not defined in the NVMe-oF specification
/// are given values outside of the 8-bit range of the TRTYPE value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportType {
    /// A RDMA Transport.
    RDMA = 1,

    /// A Fibre Channel transport.
    FC = 2,

    /// A TCP Transport.
    TCP = 3,

    /// A PCIe Transport.
    PCIE = 256,

    /// A user-mode vfio transport.
    VFIOUSER = 1024,

    /// A custom transport.
    CUSTOM = 4096,
}

impl From<TransportType> for &'static CStr {
    fn from(transport_type: TransportType) -> Self {
        unsafe {
            match transport_type {
                TransportType::CUSTOM => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_CUSTOM),
                TransportType::FC => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_FC),
                TransportType::PCIE => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_PCIE),
                TransportType::RDMA => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_RDMA),
                TransportType::TCP => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_TCP),
                TransportType::VFIOUSER => CStr::from_bytes_with_nul_unchecked(SPDK_NVME_TRANSPORT_NAME_VFIOUSER),
            }
        }
    }
}

impl From<spdk_nvme_transport_type> for TransportType {
    fn from(transport_type: spdk_nvme_transport_type) -> Self {
        match transport_type {
            SPDK_NVME_TRANSPORT_CUSTOM => TransportType::CUSTOM,
            SPDK_NVME_TRANSPORT_FC => TransportType::FC,
            SPDK_NVME_TRANSPORT_PCIE => TransportType::PCIE,
            SPDK_NVME_TRANSPORT_RDMA => TransportType::RDMA,
            SPDK_NVME_TRANSPORT_TCP => TransportType::TCP,
            SPDK_NVME_TRANSPORT_VFIOUSER => TransportType::VFIOUSER,
            _ => unreachable!("invalid transport type"),
        }
    }
}

impl Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &CStr = (*self).into();

        write!(f, "{}", s.to_string_lossy())
    }
}

/// Builds a NVMe-oF transport.
pub struct Builder {
    transport_name: &'static CStr,
    opts: Box<spdk_nvmf_transport_opts>,
}

unsafe impl Send for Builder {}

impl Builder {
    /// Creates a new builder for the given transport type.
    pub fn new(transport_type: TransportType) -> Result<Self, Errno> {
        unsafe {
            let mut opts = MaybeUninit::uninit();
            let transport_name: &CStr = transport_type.into();

            if !spdk_nvmf_transport_opts_init(
                transport_name.as_ptr(),
                opts.as_mut_ptr(),
                size_of_val(&opts)) {
                return Err(EINVAL);
            }

            Ok(Self{ transport_name, opts: Box::new(opts.assume_init()) })
        }
    }

    /// Creates the configured `Transport`.
    pub async fn build(self) -> Result<Transport, Errno> {
        Promise::new(move |cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_nvmf_transport_create_async(
                    self.transport_name.as_ptr(),
                    self.opts.as_ref() as *const _ as *mut _,
                    Some(complete_with_object::<Transport, spdk_nvmf_transport>),
                    cx))
            }
        }).await
    }

    /// Sets the maximum queue depth.
    pub fn with_max_queue_depth(mut self, max_queue_depth: u16) -> Self {
        self.opts.max_queue_depth = max_queue_depth;

        self
    }

    /// Sets the maximum number of queue pairs per controller.
    pub fn with_max_qpairs_per_ctrlr(mut self, max_qpairs_per_ctrlr: u16) -> Self {
        self.opts.max_qpairs_per_ctrlr = max_qpairs_per_ctrlr;

        self
    }

    /// Sets the maximum in capsule data size.
    pub fn with_in_capsule_data_size(mut self, in_capsule_data_size: u32) -> Self {
        self.opts.in_capsule_data_size = in_capsule_data_size;

        self
    }

    /// Sets the maximum I/O size.
    pub fn with_max_io_size(mut self, max_io_size: u32) -> Self {
        self.opts.max_io_size = max_io_size;

        self
    }

    /// Sets the I/O unit size.
    pub fn with_io_unit_size(mut self, io_unit_size: u32) -> Self {
        self.opts.io_unit_size = io_unit_size;

        self
    }

    /// Sets the maximum AQ depth.
    pub fn with_max_aq_depth(mut self, max_aq_depth: u32) -> Self {
        self.opts.max_aq_depth = max_aq_depth;

        self
    }

    /// Sets the number of shared buffers.
    pub fn with_num_shared_buffers(mut self, num_shared_buffers: u32) -> Self {
        self.opts.num_shared_buffers = num_shared_buffers;

        self
    }

    /// Sets the buffer cache size.
    pub fn with_buf_cache_size(mut self, buf_cache_size: u32) -> Self {
        self.opts.buf_cache_size = buf_cache_size;

        self
    }

    pub fn with_dif_insert_or_strip(mut self, dif_insert_or_strip: bool) -> Self {
        self.opts.dif_insert_or_strip = dif_insert_or_strip;

        self
    }

    /// Sets the abort timeout.
    pub fn with_abort_timeout(mut self, abort_timeout: Duration) -> Self {
        self.opts.abort_timeout_sec = abort_timeout.as_secs() as u32;

        self
    }

    /// Sets the association timeout.
    pub fn with_association_timeout(mut self, association_timeout: Duration) -> Self {
        self.opts.association_timeout = association_timeout.as_millis() as u32;

        self
    }

    /// Sets the listener poll rate.
    pub fn with_acceptor_poll_rate(mut self, acceptor_poll_rate: Duration) -> Self {
        self.opts.acceptor_poll_rate = acceptor_poll_rate.as_millis() as u32;

        self
    }

    /// Sets whether to enable zero-copy.
    pub fn with_zero_copy_enabled(mut self, zero_copy_enabled: bool) -> Self {
        self.opts.zcopy = zero_copy_enabled;

        self
    }
}

/// Represents the ownership state of a [`Transport`].
#[derive(Debug)]
enum OwnershipState {
    Owned(NonNull<spdk_nvmf_transport>),
    Borrowed(NonNull<spdk_nvmf_transport>),
    None,
}

unsafe impl Send for OwnershipState {}

/// Represents a NVMe-oF transport.
/// 
/// `Transport` wraps an `spdk_nvmf_transport` pointer and can be in one of
/// three ownership states: owned, borrowed, or none.
/// 
/// An owned transport owns the underlying `spdk_nvmf_transport` pointer and
/// will destroy it when dropped. The caller must ensure that the drop occurs in
/// the same thread that created the transport. It must also occur as part of
/// thread event handling by explicitly calling [`task::yield_now`] before
/// dropping the transport. However, it is easiest and safest to explicitly call
/// [`Transport::destroy`] on the transport rather than let it drop naturally.
/// 
/// A borrowed transport borrows the underlying `spdk_nvmf_transport` pointer.
/// Dropping a borrowed transport has no effect on the underlying
/// `spdk_nvmf_transport` pointer.
/// 
/// A transport with no ownership state can only be safely queried for ownership
/// state or dropped. Any other operation will panic. A transport will be left
/// in this state after the [`Transport::take`] method is called.
/// 
/// [`Transport::destroy`]: method@Transport::destroy
/// [`Transport::take`]: method@Transport::take
/// [`task::yield_now`]: function@crate::task::yield_now
#[derive(Debug)]
pub struct Transport(OwnershipState);

unsafe impl Send for Transport {}

impl Transport {
    /// Returns an owned transport from a raw `spdk_nvmf_transport` pointer.
    pub fn from_ptr_owned(ptr: *const spdk_nvmf_transport) -> Self {
        match NonNull::new(ptr as *mut spdk_nvmf_transport) {
            Some(ptr) => Self(OwnershipState::Borrowed(ptr)),
            None => panic!("transport pointer must not be null"),
        }
    }

    /// Returns a borrowed transport from a raw `spdk_nvmf_transport` pointer.
    pub fn from_ptr(ptr: *const spdk_nvmf_transport) -> Self {
        match NonNull::new(ptr as *mut spdk_nvmf_transport) {
            Some(ptr) => Self(OwnershipState::Borrowed(ptr)),
            None => panic!("transport pointer must not be null"),
        }
    }

    /// Returns a pointer to the underlying `spdk_nvmf_transport` structure.
    pub fn as_ptr(&self) -> *mut spdk_nvmf_transport {
        match &self.0 {
            OwnershipState::Owned(ptr) | OwnershipState::Borrowed(ptr) => {
                ptr.as_ptr()
            },
            OwnershipState::None => panic!("no transport"),
        }
    }

    /// Borrows the transport.
    pub fn borrow(&self) -> Self {
        match self.0 {
            OwnershipState::Owned(ptr) | OwnershipState::Borrowed(ptr) => {
                Self(OwnershipState::Borrowed(ptr))
            },
            OwnershipState::None => panic!("no transport"),
        }
    }

    /// Returns whether the transport is owned.
    pub fn is_owned(&self) -> bool {
        matches!(self.0, OwnershipState::Owned(_))
    }

    /// Returns whether the transport is borrowed.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.0, OwnershipState::Borrowed(_))
    }

    /// Returns whether this transport has no ownership state.
    pub fn is_none(&self) -> bool {
        matches!(self.0, OwnershipState::None)
    }

    /// Takes the value from this transport and replaces with a value having no
    /// ownership.
    pub fn take(&mut self) -> Self {
        mem::replace(self, Self(OwnershipState::None))
    }

    /// Returns the name of the transport.
    pub fn name(&self) -> &'static CStr {
        unsafe {
            CStr::from_ptr(spdk_nvmf_get_transport_name(self.as_ptr()))
        }
    }

    /// Returns the type of the transport.
    pub fn r#type(&self) -> TransportType {
        unsafe { spdk_nvmf_get_transport_type(self.as_ptr()).into() }
    }

    /// Destroys the transport asynchronously.
    /// 
    /// # Returns
    /// 
    /// Only an owned transport can be destroyed. This function returns
    /// `Err(EPERM)` if called on a borrowed transport and `Err(ENODEV)` if
    /// called on a transport that neither owns nor borrows the underlying
    /// `spdk_nvmf_transport` pointer.
    pub async fn destroy(mut self) -> Result<(), Errno> {
        match self.0 {
            OwnershipState::Borrowed(_) => Err(EPERM),
            OwnershipState::None => Err(ENODEV),
            OwnershipState::Owned(_) => {
                Promise::new(move |cx| {
                    unsafe {
                        let res = to_poll_pending_on_ok!(
                            spdk_nvmf_transport_destroy(
                                self.as_ptr(),
                                Some(complete_with_ok),
                                cx));

                        mem::forget(self.take());

                        res
                    }
                }).await
            },
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        if self.is_owned() {
            let transport = self.take();

            thread::block_on(async move { transport.destroy().await }).unwrap();
        }
    }
}

impl TryFrom<*mut spdk_nvmf_transport> for Transport {
    type Error = Errno;

    fn try_from(ptr: *mut spdk_nvmf_transport) -> Result<Self, Self::Error> {
        match NonNull::new(ptr) {
            Some(ptr) => Ok(Self(OwnershipState::Owned(ptr))),
            None => Err(ENOMEM),
        }
    }
}

impl TryFrom<*const spdk_nvmf_transport> for Transport {
    type Error = Errno;

    fn try_from(ptr: *const spdk_nvmf_transport) -> Result<Self, Self::Error> {
        match NonNull::new(ptr as *mut _) {
            Some(ptr) => Ok(Self(OwnershipState::Borrowed(ptr))),
            None => Err(ENOMEM),
        }
    }
}

/// An iterator over the NVMe-oF transports of a target.
pub struct Transports(*mut spdk_nvmf_transport);

unsafe impl Send for Transports {}

impl Transports {
    pub(crate) fn new(target: &Target) -> Self {
        Self(unsafe { spdk_nvmf_transport_get_first(target.as_ptr()) })
    }
}

impl Iterator for Transports {
    type Item = Transport;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            return None;
        }
        let transport = self.0;

        self.0 = unsafe { spdk_nvmf_transport_get_next(self.0) };

        Some(Transport::from_ptr(transport))
    }
}
