use std::{
    cell::RefCell,
    ffi::CStr,
    mem::{
        MaybeUninit,

        size_of_val,
    },
    ptr::NonNull,
    rc::Rc,
    task::{
        Poll,
        Waker,
    },
};

use futures::Future;
use spdk_sys::{
    Errno,
    spdk_nvmf_tgt,

    to_result,

    spdk_nvmf_get_first_tgt,
    spdk_nvmf_get_next_tgt,
    spdk_nvmf_listen_opts_init,
    spdk_nvmf_subsystem_create,
    spdk_nvmf_tgt_add_transport,
    spdk_nvmf_tgt_get_name,
    spdk_nvmf_tgt_listen_ext,
};

use crate::{
    errors::ENOMEM,
    nvme::TransportId,
    nvmf::SPDK_NVMF_DISCOVERY_NQN,
    thread::Thread,
};

use super::{Transport, Subsystem, subsystem::{Subsystems, SubsystemType}, transport::Transports};

/// Encapsulates the state of adding a transport to a NVMe-oF target.
#[derive(Default)]
struct AddTransportState {
    result: Option<Result<(), Errno>>,
    waker: Option<Waker>,
}

impl AddTransportState {
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

/// Orchestrates adding a transport to a NVMe-oF target.
struct AddTransport<'a> {
    target: &'a Target,
    transport: Transport,
    state: Rc<RefCell<AddTransportState>>,
}

impl AddTransport<'_> {
    /// A callback invoked with the result of adding a transport to a NVMe-oF target.
    unsafe extern "C" fn complete(
        cx: *mut std::ffi::c_void,
        status: i32,
    ) {
        let state = Rc::from_raw(cx as *mut RefCell<AddTransportState>);

        let waker = match state.try_borrow_mut() {
            Ok(mut state) => state.set_result(status),
            Err(_) => {
                let state = Rc::clone(&state);

                Thread::current().send_msg(move || {
                    let waker = state.borrow_mut().set_result(status);

                    if let Some(w) = waker {
                        w.wake();
                    }
                }).expect("send result");

                None
            }
            
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Future for AddTransport<'_> {
    type Output = Result<(), Errno>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let mut state = self.state.borrow_mut();

        match state.result.take() {
            Some(Ok(())) => Poll::Ready(Ok(())),
            Some(Err(e)) => Poll::Ready(Err(e)),
            None => {
                state.waker = Some(cx.waker().clone());

                let state_raw = Rc::as_ptr(&self.state);

                unsafe {
                    Rc::increment_strong_count(state_raw);

                    spdk_nvmf_tgt_add_transport(
                        self.target.as_ptr(),
                        self.transport.as_ptr() as *mut _,
                        Some(Self::complete),
                        state_raw as *mut std::ffi::c_void,
                    );
                }

                Poll::Pending
            },
        }
    }
}

/// Represents a NVMe-oF target.
pub struct Target(NonNull<spdk_nvmf_tgt>);

unsafe impl Send for Target {}

impl Target {
    /// Returns the name of the target.
    pub fn name(&self) -> &'static CStr {
        unsafe {
            let name = spdk_nvmf_tgt_get_name(self.as_ptr());

            CStr::from_ptr(name)
        }
    }

    /// Returns a NVMf target from a raw `spdk_nvmf_target` pointer.
    pub fn from_ptr(ptr: *mut spdk_nvmf_tgt) -> Self {
        match NonNull::new(ptr) {
            Some(ptr) => Self(ptr),
            None => panic!("target pointer must not be null"),
        }
    }

    /// Returns a pointer to the underlying `spdk_nvmf_target` structure.
    pub fn as_ptr(&self) -> *mut spdk_nvmf_tgt {
        self.0.as_ptr()
    }

    /// Adds a transport to the target.
    pub async fn add_transport(&mut self, transport: Transport) -> Result<(), Errno> {
        AddTransport{
            target: self,
            transport,
            state: Rc::new(RefCell::new(AddTransportState::default())),
        }.await
    }

    /// Returns an iterator over the transports on this target.
    pub fn transports(&self) -> Transports {
        Transports::new(self)
    }

    /// Enables the NVMe-oF Discovery Controller subsystem on this target.
    pub fn enable_discovery(&mut self) -> Result<Subsystem, Errno> {
        let discovery = self.add_subsystem(
            SPDK_NVMF_DISCOVERY_NQN,
            SubsystemType::Discovery,
            0)?;
        
        discovery.allow_any_host(true);
        Ok(discovery)
    }

    /// Adds a subsystem to the target.
    /// 
    /// A subsystem is a collection of namespaces that are exported over
    /// NVMe-oF. It can be in one of three states: Inactive, Active, or Paused.
    /// This state affects which operations may be perform on the subsystem. On
    /// creation, the subsystem is in the Inactive state and may be activated by
    /// calling the subsystem's [`start`] method. No I/O will be processed in
    /// the Inactive or Paused states but changes to the state of the subsystem
    /// may be made.
    /// 
    /// [`start`]: method@Subsystem::start
    pub fn add_subsystem(&mut self, nqn: &CStr, subtype: SubsystemType, num_ns: u32) -> Result<Subsystem, Errno> {
        let subsys = unsafe {
            spdk_nvmf_subsystem_create(self.as_ptr(), nqn.as_ptr(), subtype.into(), num_ns)
        };

        if subsys.is_null() {
            return Err(ENOMEM);
        }

        Ok(Subsystem(subsys))
    }

    /// Returns an iterator over the subsystems on this target.
    pub fn subsystems(&self) -> Subsystems {
        Subsystems::new(self)
    }

    /// Starts the subsystems on this target.
    pub async fn start_subsystems(&self) -> Result<(), Errno> {
        for subsys in self.subsystems() {
            subsys.start().await?;
        }

        Ok(())
    }

    /// Stops the subsystems on this target.
    pub async fn stop_subsystems(&self) -> Result<(), Errno> {
        for subsys in self.subsystems() {
            subsys.stop().await?;
        }

        Ok(())
    }

    /// Pauses the subsystems on this target.
    pub async fn pause_subsystems(&self, ns: u32) -> Result<(), Errno> {
        for subsys in self.subsystems() {
            subsys.pause(ns).await?;
        }

        Ok(())
    }

    /// Resumes the subsystems on this target.
    pub async fn resume_subsystems(&self) -> Result<(), Errno> {
        for subsys in self.subsystems() {
            subsys.resume().await?;
        }

        Ok(())
    }

    /// Begins accepting new connections on the specified transport.
    pub fn listen(&self, transport_id: &TransportId) -> Result<(), Errno> {
        unsafe {
            let mut opts = MaybeUninit::uninit();

            spdk_nvmf_listen_opts_init(opts.as_mut_ptr(), size_of_val(&opts));

            let mut opts = opts.assume_init();

            to_result!(spdk_nvmf_tgt_listen_ext(self.as_ptr(), transport_id.as_ptr(), &mut opts as *mut _))
        }
    }
}

/// An iterator over the NVMe-oF targets.
pub struct Targets(*mut spdk_nvmf_tgt);

unsafe impl Send for Targets {}

impl Iterator for Targets {
    type Item = Target;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            None
        } else {
            unsafe {
                let tgt = self.0;

                self.0 = spdk_nvmf_get_next_tgt(tgt);

                Some(Target::from_ptr(tgt))
            }
        }
    }
}

/// Get an iterator over the NVMe-oF targets.
pub fn targets() -> Targets {
    Targets(unsafe { spdk_nvmf_get_first_tgt() })
}
