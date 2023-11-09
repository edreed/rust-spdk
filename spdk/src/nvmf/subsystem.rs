use std::{
    cell::RefCell,
    mem::ManuallyDrop,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    ptr::{null_mut, NonNull}, ffi::{CStr, c_void},
};

use futures::Future;
use spdk_sys::{
    Errno,
    spdk_nvmf_subsystem,
    spdk_nvmf_subtype,
    
    SPDK_NVMF_SUBTYPE_DISCOVERY,
    SPDK_NVMF_SUBTYPE_NVME,

    to_result,

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
        EINVAL,
        ENOENT,
    },
    nvme::TransportId,
    thread::Thread,
};

use super::{Namespace, Target, AllowedHosts, Namespaces};

/// Encapsulates the state of a NVMe-oF  operation.
#[derive(Default)]
struct TaskState {
    result: Option<Result<(), Errno>>,
    waker: Option<Waker>,
}

impl TaskState {
    /// Sets the result and returns a [`Waker`] to awaken the waiting future.
    fn set_result(&mut self, status: i32) -> Option<Waker> {
        if status < 0 {
            self.result = Some(Err(Errno(-status)));
        } else {
            self.result = Some(Ok(()));
        }

        self.waker.take()
    }
}

/// A function that starts a subsystem operation.
type SubsystemFn = unsafe fn(data: *const ()) -> Result<(), Errno>;

enum Operation<'a> {
    Start,
    Stop,
    Pause {
        ns: u32
    },
    Resume,
    AddListener {
        transport_id: &'a TransportId,
    },
}

/// Orchestrates a NVMe-oF  operation.
struct TaskInner<'a> {
    subsystem: &'a Subsystem,
    op: Operation<'a>,
    subsystem_fn: SubsystemFn,
    state: RefCell<TaskState>,
}

impl TaskInner<'_> {
    /// A callback invoked with the result of a NVMe-oF 
    /// operation.
    unsafe extern "C" fn complete(cx: *mut c_void, status: i32) {
        let this = Rc::from_raw(cx as *mut TaskInner);

        let waker = match this.state.try_borrow_mut() {
            Ok(mut state) => state.set_result(status),
            Err(_) => {
                let this = Rc::clone(&this);

                Thread::current().send_msg(move || {
                    let waker = this.state.borrow_mut().set_result(status);

                    if let Some(w) = waker {
                        w.wake();
                    }
                }).expect("send result");

                None
            }
        };

        if let Some(w) = waker {
            w.wake();
        }
    }

    /// A callback invoked with the result of a NVMe-oF  state
    /// change operation.
    unsafe extern "C" fn state_change_complete(
        _subsys: *mut spdk_nvmf_subsystem,
        cx: *mut c_void,
        status: i32,
    ) {
        Self::complete(cx, status);
    }

    /// Starts the subsystem.
    /// 
    /// This method begins the transition of the subsystem from the Inactive to Active state.
    unsafe fn start(data: *const ()) -> Result<(), Errno> {
        let this = ManuallyDrop::new(Rc::from_raw(data as *const TaskInner));

        to_result!(spdk_nvmf_subsystem_start(this.subsystem.as_ptr(), Some(Self::state_change_complete), data as *mut c_void))
    }

    /// Stops the subsystem.
    /// 
    /// This method begins the transition of the subsystem from the Active to Inactive state.
    unsafe fn stop(data: *const ()) -> Result<(), Errno> {
        let this = ManuallyDrop::new(Rc::from_raw(data as *const TaskInner));

        to_result!(spdk_nvmf_subsystem_stop(this.subsystem.as_ptr(), Some(Self::state_change_complete), data as *mut c_void))
    }

    /// Pauses the subsystem.
    /// 
    /// This method begins the transition of the subsystem from the Active to Paused state.
    unsafe fn pause(data: *const ()) -> Result<(), Errno> {
        let this = ManuallyDrop::new(Rc::from_raw(data as *const TaskInner));

        if let Operation::Pause { ns } = this.op {
            return to_result!(spdk_nvmf_subsystem_pause(this.subsystem.as_ptr(), ns, Some(Self::state_change_complete), data as *mut c_void));
        }

        unreachable!("parameter mismatch");
    }

    /// Resumes the subsystem.
    /// 
    /// This method begins the transition of the subsystem from the Paused to Active state.
    unsafe fn resume(data: *const ()) -> Result<(), Errno> {
        let this = ManuallyDrop::new(Rc::from_raw(data as *const TaskInner));

        to_result!(spdk_nvmf_subsystem_resume(this.subsystem.as_ptr(), Some(Self::state_change_complete), data as *mut c_void))
    }

    unsafe fn add_listener(data: *const ()) -> Result<(), Errno> {
        let this = ManuallyDrop::new(Rc::from_raw(data as *const TaskInner));

        if let Operation::AddListener { transport_id } = this.op {
            spdk_nvmf_subsystem_add_listener(
                this.subsystem.as_ptr(),
                transport_id.as_ptr() as *mut _,
                Some(Self::complete),
                data as *mut c_void);

            return Ok(());
        }

        unreachable!("parameter mismatch");
    }
}

/// Represents an asynchronous NVMe-oF  operation.
struct Task<'a> {
    inner: Rc<TaskInner<'a>>,
}

unsafe impl Send for Task<'_> {}

impl Future for Task<'_> {
    type Output = Result<(), Errno>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
        let mut state = self.inner.state.borrow_mut();

        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker = Some(cx.waker().clone());

                unsafe {
                    let inner_raw = Rc::as_ptr(&self.inner);

                    Rc::increment_strong_count(inner_raw);

                    if let Err(e) = (self.inner.subsystem_fn)(inner_raw as *const ()) {
                        Rc::decrement_strong_count(inner_raw);

                        return Poll::Ready(Err(e));
                    }
                }

                Poll::Pending
            }
        }
    }
}

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

    /// Starts the subsystem.
    /// 
    /// This method transitions of the subsystem from the Inactive to Active state.
    pub async fn start(&self) -> Result<(), Errno> {
        Task {
            inner: Rc::new(TaskInner {
                subsystem: self,
                op: Operation::Start,
                subsystem_fn: TaskInner::start,
                state: RefCell::new(Default::default()),
            }),
        }.await
    }

    /// Stops the subsystem.
    /// 
    /// This method transitions of the subsystem from the Active to Inactive state.
    pub async fn stop(&self) -> Result<(), Errno> {
        Task {
            inner: Rc::new(TaskInner {
                subsystem: self,
                op: Operation::Stop,
                subsystem_fn: TaskInner::stop,
                state: RefCell::new(Default::default()),
            }),
        }.await
    }

    /// Pauses the subsystem.
    /// 
    /// This method transitions of the subsystem from the Paused to Inactive state.
    pub async fn pause(&self, ns: u32) -> Result<(), Errno> {
        Task {
            inner: Rc::new(TaskInner {
                subsystem: self,
                op: Operation::Pause { ns },
                subsystem_fn: TaskInner::pause,
                state: RefCell::new(Default::default()),
            }),
        }.await
    }

    /// Resumes the subsystem.
    /// 
    /// This method transitions of the subsystem from the Inactive to Paused state.
    pub async fn resume(&self) -> Result<(), Errno> {
        Task {
            inner: Rc::new(TaskInner {
                subsystem: self,
                op: Operation::Resume,
                subsystem_fn: TaskInner::resume,
                state: RefCell::new(Default::default()),
            }),
        }.await
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
    
            return Ok(Namespace(spdk_nvmf_subsystem_get_ns(self.as_ptr(), nsid)));
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
        Task {
            inner: Rc::new(TaskInner {
                subsystem: self,
                op: Operation::AddListener { transport_id },
                subsystem_fn: TaskInner::add_listener,
                state: RefCell::new(Default::default()),
            }),
        }.await
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