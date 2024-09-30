use std::{
    ffi::CStr,
    io::Write,
    mem::{
        self,

        MaybeUninit,

        size_of_val,
    },
    ptr::{
        NonNull,
        
        copy_nonoverlapping,
    },
    task::Poll,
};

use spdk_sys::{
    spdk_nvmf_target_opts,
    spdk_nvmf_tgt,
    spdk_nvmf_tgt_discovery_filter,

    SPDK_NVMF_DISCOVERY_NQN,

    spdk_nvmf_get_first_tgt,
    spdk_nvmf_get_next_tgt,
    spdk_nvmf_listen_opts_init,
    spdk_nvmf_subsystem_create,
    spdk_nvmf_subsystem_destroy,
    spdk_nvmf_tgt_add_transport,
    spdk_nvmf_tgt_create,
    spdk_nvmf_tgt_destroy,
    spdk_nvmf_tgt_get_name,
    spdk_nvmf_tgt_listen_ext,
};

use crate::{
    errors::{
        Errno,

        EBADF,
        EINPROGRESS,
        ENOMEM,
        EPERM,
    },
    nvme::{
        SPDK_NVME_GLOBAL_NS_TAG,

        TransportId,
    },
    task::{
        Promise,
        Promissory,
    },
    thread,
    to_poll_pending_on_err,
    to_result,
};

use super::{
    Subsystem,
    Transport,

    subsystem::{
        Subsystems,
        SubsystemType,
    },
    transport::Transports,
};

/// Builds a [`Target`] instance using the NVMe over Fabrics (NVMe-oF) target
/// module of the SPDK.
/// 
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chaining function calls. The [`build`] method constructs a new
/// `Target` instance.
/// 
/// [`build`]: Builder::build
pub struct Builder(spdk_nvmf_target_opts);

impl Builder {
    /// Creates a new builder for a NVMe-oF target.
    pub fn new(name: &str) -> Self {
        unsafe {
            let mut opts: spdk_nvmf_target_opts = MaybeUninit::zeroed().assume_init();

            write!(
                &mut *(&mut opts.name[..] as *mut _ as *mut [u8]),
                "{}",
                name,
            ).expect("name less than 255 characters");

            Self(opts)
        }
    }

    /// Creates a new [`Target`] instance that owns the underlying
    /// `spdk_nvmf_tgt` pointer.
    pub fn build(self) -> Result<Target, Errno> {
        unsafe {
            match NonNull::new(spdk_nvmf_tgt_create(&self.0 as *const _ as *mut _)) {
                Some(ptr) => Ok(Target(OwnershipState::Owned(ptr))),
                None => Err(ENOMEM),
            }
        }
    }

    /// Sets the maximum number of subsystems.
    pub fn with_max_subsystems(mut self, max_subsystems: u32) -> Self {
        self.0.max_subsystems = max_subsystems;
        self
    }

    /// Sets the Command Retry Delay Times (CRDTs).
    pub fn with_command_retry_delay_times(mut self, crdts: [u16; 3]) -> Self {
        unsafe {
            copy_nonoverlapping(
                crdts.as_ptr(),
                self.0.crdt.as_mut_ptr(),
                crdts.len(),
            );
        }
        self
    }

    /// Sets the filter rule applied during discovery log generation.
    pub fn with_discovery_filter(mut self, filter: spdk_nvmf_tgt_discovery_filter) -> Self {
        self.0.discovery_filter = filter;
        self
    }
}

/// Represents the ownership state of a NVMe-oF target.
enum OwnershipState {
    Owned(NonNull<spdk_nvmf_tgt>),
    Borrowed(NonNull<spdk_nvmf_tgt>),
    None,
}

unsafe impl Send for OwnershipState {}

/// Represents a NVMe-oF target.
/// 
/// `Target` wraps an `spdk_nvmf_tgt` pointer and can be in one of three
/// ownership states: owned, borrowed, or none.
/// 
/// An owned transport owns the underlying `spdk_nvmf_tgt` pointer and will
/// destroy it when dropped. The caller must ensure that the drop occurs in the
/// same thread that created the transport. It must also occur as part of thread
/// event handling by explicitly calling [`task::yield_now`] before dropping the
/// transport. However, it is easiest and safest to explicitly call
/// [`Target::destroy`] on the transport rather than let it drop naturally.
/// 
/// A borrowed transport borrows the underlying `spdk_nvmf_tgt` pointer.
/// Dropping a borrowed transport has no effect on the underlying
/// `spdk_nvmf_tgt` pointer.
/// 
/// A transport with no ownership state can only be safely queried for ownership
/// state or dropped. Any other operation will panic. A transport will be left
/// in this state after the [`Target::take`] method is called.
/// 
/// [`Target::destroy`]: method@Target::destroy
/// [`Target::take`]: method@Target::take
/// [`task::yield_now`]: function@crate::task::yield_now
pub struct Target(OwnershipState);

unsafe impl Send for Target {}

impl Target {
    /// Creates a new NVMe-oF target that owns the underlying `spdk_nvmf_tgt`
    /// pointer.
    pub fn new(name: &str) -> Result<Self, Errno> {
        Builder::new(name).build()
    }

    /// Returns the name of the target.
    pub fn name(&self) -> &'static CStr {
        unsafe {
            let name = spdk_nvmf_tgt_get_name(self.as_ptr());

            CStr::from_ptr(name)
        }
    }

    /// Returns a borrowed NVMf target from a raw `spdk_nvmf_target` pointer.
    pub fn from_ptr(ptr: *mut spdk_nvmf_tgt) -> Self {
        match NonNull::new(ptr) {
            Some(ptr) => Self(OwnershipState::Borrowed(ptr)),
            None => panic!("target pointer must not be null"),
        }
    }

    /// Returns a pointer to the underlying `spdk_nvmf_target` structure.
    pub fn as_ptr(&self) -> *mut spdk_nvmf_tgt {
        match self.0 {
            OwnershipState::Owned(ptr) | OwnershipState::Borrowed(ptr) => {
                ptr.as_ptr()
            },
            OwnershipState::None => panic!("target pointer must not be null"),
        }
    }

    /// Borrows the NVMe-oF target.
    pub fn borrow(&self) -> Self {
        match self.0 {
            OwnershipState::Owned(ptr) | OwnershipState::Borrowed(ptr) => {
                Self(OwnershipState::Borrowed(ptr))
            },
            OwnershipState::None => panic!("no target"),
        }
    }

    /// Returns whether this target is owned.
    pub fn is_owned(&self) -> bool {
        matches!(self.0, OwnershipState::Owned(_))
    }

    /// Returns whether this target is borrowed.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.0, OwnershipState::Borrowed(_))
    }

    /// Returns whether this target has no ownership state.
    pub fn is_none(&self) -> bool {
        matches!(self.0, OwnershipState::None)
    }

    /// Takes the value from this NVMe-oF target and replaces with a value
    /// having no ownership.
    pub fn take(&mut self) -> Self {
        mem::replace(self, Target(OwnershipState::None))
    }

    /// Destroy the NVMe-oF target asynchronously.
    /// 
    /// # Returns
    /// 
    /// Only an owned target can be destroyed. This function returns
    /// `Err(EPERM)` if called on a borrowed target and `Err(ENODEV)` if called
    /// on a target that neither owns nor borrows the underlying `spdk_nvmf_tgt`
    /// pointer.
    pub async fn destroy(mut self) -> Result<(), Errno> {
        match self.0 {
            OwnershipState::Owned(_) => {
                Promise::new(move |p| {
                    let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

                    unsafe {
                        spdk_nvmf_tgt_destroy(
                            self.as_ptr(),
                            Some(cb_fn),
                            cb_arg.cast_mut() as *mut _,
                        );
                    }

                    mem::forget(self.take());

                    Poll::Pending
                }).await
            },
            OwnershipState::Borrowed(_) => Err(EPERM),
            OwnershipState::None => Err(EBADF),
        }
    }

    /// Adds a transport to the target.
    pub async fn add_transport(&mut self, transport: Transport) -> Result<(), Errno> {
        let res = Promise::new(|p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

            unsafe {
                spdk_nvmf_tgt_add_transport(
                    self.as_ptr(),
                    transport.as_ptr(),
                    Some(cb_fn),
                    cb_arg.cast_mut() as *mut _,
                );
            }

            Poll::Pending
        }).await;

        match res {
            Ok(()) => {
                // The transport is now owned by the target, so we forget it to avoid
                // destroying it.
                mem::forget(transport);

                Ok(())
            },
            Err(e) => {
                // Since adding the transport failed, explicitly destroy it
                // rather then let it drop to avoid blocking current reactor from
                // executing other tasks.
                transport.destroy().await.expect("transport destroyed");

                Err(e)
            },
        }
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

        Ok(Subsystem::from_ptr(subsys))
    }

    /// Removes a subsystem from the target.
    pub async fn remove_subsystem(&mut self, subsys: Subsystem) -> Result<(), Errno> {
        Promise::new(|p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_ok(p);

            to_poll_pending_on_err!{
                EINPROGRESS,
                unsafe {
                    spdk_nvmf_subsystem_destroy(
                        subsys.as_ptr(),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        }).await
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
    pub async fn pause_subsystems(&self) -> Result<(), Errno> {
        for subsys in self.subsystems() {
            subsys.pause(SPDK_NVME_GLOBAL_NS_TAG).await?;
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

impl Drop for Target {
    fn drop(&mut self) {
        if self.is_owned() {
            let tgt = self.take();

            thread::block_on(async move { tgt.destroy().await }).unwrap();
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
