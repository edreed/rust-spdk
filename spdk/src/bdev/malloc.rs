//! Support for the Storage Performance Development Kit Malloc Block Device
//! plug-in.
use std::{
    default::Default,
    ffi::CStr,
    future::Future,
    mem::zeroed,
    os::raw:: {
        c_char,
        c_void,
    },
    pin::Pin,
    ptr::null_mut,
    sync::{
        Arc,
        Mutex,
    },
    task::{
        Context,
        Poll, Waker,
    },
};

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
    block::{
        Device,
        Descriptor,
    },
    thread::Thread,
};

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

    /// Creates a new [`Malloc`] instance.
    pub fn build(self) -> Result<Malloc, Errno>{
        let mut malloc = null_mut();
        
        unsafe { to_result!(create_malloc_disk(&mut malloc, &self.0))? };

        Ok(Malloc(malloc))
    }
}

impl Default for Builder {
    fn default() -> Self {
        unsafe { zeroed::<Self>() }
    }
}

struct DeleteMallocState {
    bdev: *mut spdk_bdev,
    waker: Option<Waker>,
    result: Option<Result<(), Errno>>,
}

unsafe impl Send for DeleteMallocState {}

/// Encapsulates the state of an asynchronous [`Malloc`] delete operation.
impl DeleteMallocState {
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
struct DeleteMalloc {
    inner: Arc<Mutex<DeleteMallocState>>
}

unsafe impl Send for DeleteMalloc {}

impl DeleteMalloc {
    /// A callback invoked with the result of the asynchronous delete operation.
    unsafe extern "C" fn complete(arg: *mut c_void, status: i32) {
        let inner = Arc::from_raw(arg as *mut Mutex<DeleteMallocState>);
        let waker = match inner.try_lock() {
            Ok(mut state) => state.set_result(status),
            Err(_) => {
                let inner = inner.clone();

                Thread::current().send_msg(move || {
                    let waker = inner.lock().unwrap().set_result(status);

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

impl Future for DeleteMalloc {
    type Output = Result<(), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.lock().unwrap();

        if let Some(result) = inner.result.take() {
            return Poll::Ready(result);
        }

        inner.waker = Some(cx.waker().clone());

        unsafe {
            delete_malloc_disk(
                spdk_bdev_get_name(inner.bdev),
                Some(Self::complete),
                Arc::into_raw(self.inner.clone()) as *mut c_void)
        };

        Poll::Pending
    }
}

/// Represents a Malloc Block Device.
pub struct Malloc(*mut spdk_bdev);

unsafe impl Send for Malloc {}

impl Malloc {
    /// Returns the name of the device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_bdev_get_name(self.0)) }
    }

    /// Returns a [`Device`] representing the underlying SPDK block device.
    pub fn to_block_device(&self) -> Device {
        Device::from_bdev(self.0)
    }

    /// Opens the device asynchronously.
    pub async fn open(&self, write: bool) -> Result<Descriptor, Errno> {
        Descriptor::open(self.name(), write).await
    }

    /// Deletes the device asynchronously.
    pub async fn delete(&mut self) -> Result<(), Errno> {
        if self.0.is_null() {
            return Ok(());
        }

        let inner = Arc::new(Mutex::new(DeleteMallocState {
            bdev: self.0,
            waker: None,
            result: None,
        }));

        self.0 = null_mut();

        DeleteMalloc { inner }.await
    }
}

impl Drop for Malloc {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { delete_malloc_disk(spdk_bdev_get_name(self.0), None, null_mut()) };
        }
    }
}
