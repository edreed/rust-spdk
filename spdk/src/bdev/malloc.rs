//! Support for the Storage Performance Development Kit Malloc Block Device
//! plug-in.
use std::{
    cell::RefCell,
    default::Default,
    ffi::CStr,
    future::Future,
    mem::{
        self,

        ManuallyDrop,
    },
    os::raw:: {
        c_char,
        c_void,
    },
    pin::Pin,
    ptr::null_mut,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};

use async_trait::async_trait;
use spdk_sys::{
    Errno,
    malloc_bdev_opts,
    spdk_bdev,

    to_result,

    create_malloc_disk,
    delete_malloc_disk, 
    spdk_bdev_get_name,
};

use crate::{
    block::Device,
    errors::EBADF,
    thread::Thread,
};

use super::BDev;

/// Builds a [`Malloc`] instance using the Malloc Block Device module of the
/// SPDK.
/// 
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chaining function calls. The [`build`] method constructs a new
/// `Malloc` instance.
/// 
/// [`build`]: Builder::build
pub  struct Builder(malloc_bdev_opts);

unsafe impl Send for Builder {}

impl Builder {
    /// Returns a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the device name.
    pub fn with_name(mut self, name: &CStr) -> Self {
        self.0.name = name.as_ptr() as *mut c_char;
        self
    }

    /// Sets the total capacity of the device in blocks.
    pub fn with_num_blocks(mut self, num_blocks: u64) -> Self {
        self.0.num_blocks = num_blocks;
        self
    }

    /// Sets the block size, in bytes.
    pub fn with_block_size(mut self, block_size: u32) -> Self {
        self.0.block_size = block_size;
        self
    }

    /// Creates a new [`Device<Malloc>`] instance that owns the underlying
    /// `spdk_bdev` pointer.
    /// 
    /// # Notes
    /// 
    /// The returned [`Device<Malloc>`] instance owns the underlying `spdk_bdev`
    /// pointer and will destroy it when dropped. See [`Device<T>`] for a detailed
    /// discussion of ownership semantics and requirements.
    /// 
    /// [`Device<Malloc>::destroy`]: method@crate::block::Device<Malloc>::destroy
    /// [`task::yield_now`]: function@crate::task::yield_now
    pub fn build(self) -> Result<Device<Malloc>, Errno>{
        let mut malloc = null_mut();
        
        unsafe { to_result!(create_malloc_disk(&mut malloc, &self.0))? };

        Ok(Device::from_ptr_owned(malloc))
    }
}

impl Default for Builder {
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

#[derive(Default)]
struct DestroyState {
    waker: Option<Waker>,
    result: Option<Result<(), Errno>>,
}

unsafe impl Send for DestroyState {}

/// Encapsulates the state of an asynchronous [`Malloc`] delete operation.
impl DestroyState {
    /// Sets the result to the waiting future and returns a [`Waker`] to awaken
    /// the waiting future.
    fn set_result(&mut self, status: i32) -> Option<Waker> {
        self.result = if status == 0 {
            Some(Ok(()))
        } else {
            Some(Err(Errno(-status)))
        };

        self.waker.take()
    }
}

/// Represents an asynchronous [`Malloc`] delete operation.
struct Destroy {
    bdev: *mut spdk_bdev,
    state: Rc<RefCell<DestroyState>>
}

unsafe impl Send for Destroy {}

impl Destroy {
    /// Creates a new [`Destroy`] instance tp asynchronously destroy the
    /// specified [`Malloc`] block device.
    fn new(malloc: Malloc) -> Result<Self, Errno> {
        let mut malloc = ManuallyDrop::new(malloc);
        let bdev = mem::replace(&mut malloc.0, null_mut());

        if bdev.is_null() {
            return Err(EBADF);
        }

        Ok(Self {
            bdev,
            state: Rc::new(RefCell::new(Default::default())),
        })
    }

    /// A callback invoked with the result of the asynchronous delete operation.
    unsafe extern "C" fn complete(arg: *mut c_void, status: i32) {
        let inner = Rc::from_raw(arg as *mut RefCell<DestroyState>);
        let waker = match inner.try_borrow_mut() {
            Ok(mut state) => state.set_result(status),
            Err(_) => {
                let inner = inner.clone();

                Thread::current().send_msg(move || {
                    let waker = inner.borrow_mut().set_result(status);

                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }).expect("send result");

                None
            },
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Future for Destroy {
    type Output = Result<(), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.state.borrow_mut();

        if let Some(result) = inner.result.take() {
            return Poll::Ready(result);
        }

        inner.waker = Some(cx.waker().clone());

        unsafe {
            delete_malloc_disk(
                spdk_bdev_get_name(self.bdev),
                Some(Self::complete),
                Rc::into_raw(self.state.clone()) as *mut c_void)
        };

        Poll::Pending
    }
}

/// Represents a Malloc Block Device.
pub struct Malloc(*mut spdk_bdev);

unsafe impl Send for Malloc {}

#[async_trait]
impl BDev for Malloc {
    async fn destroy(self) -> Result<(), Errno> {
        if self.0.is_null() {
            return Ok(());
        }

        Destroy::new(self)?.await
    }
}

impl From<Device<Malloc>> for Malloc {
    fn from(device: Device<Malloc>) -> Self {
        Self(ManuallyDrop::new(device).as_ptr())
    }
}
