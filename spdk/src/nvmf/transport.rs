use std::{
    cell::RefCell,
    ffi::CStr,
    fmt::Display,
    mem::{
        MaybeUninit,

        size_of_val,
    },
    os::raw::c_void,
    pin::Pin,
    ptr::NonNull,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

use futures::Future;
use spdk_sys::{
    Errno,
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

    to_result,

    spdk_nvmf_get_transport_name,
    spdk_nvmf_get_transport_type,
    spdk_nvmf_transport_create_async,
    spdk_nvmf_transport_get_first,
    spdk_nvmf_transport_get_next,
    spdk_nvmf_transport_opts_init,
};

use crate::{
    errors::{
        EINVAL,
        ENOMEM,
    },
    thread::Thread,
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

/// Encapsulates the state of creating a NVMe-oF transport.
#[derive(Default)]
struct CreateTransportState {
    result: Option<Result<Transport, Errno>>,
    waker: Option<Waker>,
}

impl CreateTransportState {
    fn set_result(&mut self, transport: *mut spdk_nvmf_transport) -> Option<Waker> {
        if transport.is_null() {
            self.result = Some(Err(ENOMEM));
        } else {
            self.result = Some(Ok(Transport::from_ptr(transport)));
        }

        self.waker.take()
    }
}

/// Orchestrates creating a NVMe-oF transport.
struct CreateTransport {
    transport_name: &'static CStr,
    opts: Box<spdk_nvmf_transport_opts>,
    state: Rc<RefCell<CreateTransportState>>,
}

unsafe impl Send for CreateTransport {}

impl CreateTransport {
    /// A callback invoked with the result of creating a NVMe-oF transport.
    unsafe extern "C" fn complete(
        cx: *mut std::ffi::c_void,
        transport: *mut spdk_nvmf_transport,
    ) {
        let state = Rc::from_raw(cx as *const RefCell<CreateTransportState>);

        let waker = match state.try_borrow_mut() {
            Ok(mut state) => state.set_result(transport),
            Err(_) => {
                let state = Rc::clone(&state);

                Thread::current().send_msg(move || {
                    let waker = state.borrow_mut().set_result(transport);

                    if let Some(w) = waker {
                        w.wake();
                    }
                }).expect("send result");

                None
            },
        };

        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl Future for CreateTransport {
    type Output = Result<Transport, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.borrow_mut();

        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker = Some(cx.waker().clone());

                let state_raw = Rc::as_ptr(&self.state);

                unsafe {
                    Rc::increment_strong_count(state_raw);

                    let rc = 
                        to_result!(spdk_nvmf_transport_create_async(
                            self.transport_name.as_ptr(),
                            self.opts.as_ref() as *const _ as *mut _,
                            Some(Self::complete),
                            state_raw as *mut c_void));

                    if let Err(e) = rc {
                        Rc::decrement_strong_count(state_raw);

                        return Poll::Ready(Err(e));
                    }

                    Poll::Pending
                }
            }
        }
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
        CreateTransport {
            transport_name: self.transport_name,
            opts: self.opts,
            state: Default::default()
        }.await
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

/// Represents a NVMe-oF transport.
pub struct Transport(NonNull<spdk_nvmf_transport>);

unsafe impl Send for Transport {}

impl Transport {
    /// Returns a transport from a raw `spdk_nvmf_transport` pointer.
    pub fn from_ptr(ptr: *const spdk_nvmf_transport) -> Self {
        match NonNull::new(ptr as *mut spdk_nvmf_transport) {
            Some(ptr) => Self(ptr),
            None => panic!("transport pointer must not be null"),
        }
    }

    /// Returns a pointer to the underlying `spdk_nvmf_transport` structure.
    pub fn as_ptr(&self) -> *mut spdk_nvmf_transport {
        self.0.as_ptr()
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
