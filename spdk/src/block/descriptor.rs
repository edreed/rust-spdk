use std::{
    ffi::{CStr, c_void},
    marker::PhantomData,
    mem::transmute,
    ptr::{NonNull, null_mut},
};

#[cfg(feature = "bdev-module")]
use spdk_sys::spdk_bdev_module_claim_bdev_desc;
use spdk_sys::{
    spdk_bdev, spdk_bdev_close, spdk_bdev_desc, spdk_bdev_desc_get_bdev, spdk_bdev_event_type,
    spdk_bdev_get_io_channel, spdk_bdev_open_ext,
};

#[cfg(feature = "bdev-module")]
use crate::bdev::{Module, ModuleInstance, ModuleOps};
use crate::{Result, errors::ENOMEM, to_result};

#[cfg(feature = "bdev-module")]
use super::ClaimType;
use super::{Device, IoChannel, OwnedBy};

/// Represents the types of events that can occur on a block device descriptor.
#[repr(i32)]
#[non_exhaustive]
pub enum EventType {
    /// The block device has been removed. Any open descriptors must be closed.
    Remove,

    /// The block device has been resized.
    Resize,

    /// The block device has media management events awaiting processing.
    MediaManagement,
}

impl From<spdk_bdev_event_type> for EventType {
    fn from(event: spdk_bdev_event_type) -> Self {
        unsafe { transmute(event as i32) }
    }
}

impl From<EventType> for spdk_bdev_event_type {
    fn from(event: EventType) -> Self {
        unsafe { transmute(event as i32) }
    }
}

/// A trait for handling events on a block device descriptor.
pub trait EventHandler {
    fn handle_event(&self, event: EventType, device: &Device<OwnedBy<'_, Self>>);
}

/// A default no-op event handler.
impl EventHandler for () {
    fn handle_event(&self, _event: EventType, _device: &Device<OwnedBy<'_, Self>>) {
        // No-op
    }
}

/// Callback for handling descriptor events.
///
/// This callback is guaranteed to be called on the SPDK thread on which the descriptor was opened.
extern "C" fn handle_desc_event<T: EventHandler>(
    event: spdk_bdev_event_type,
    bdev: *mut spdk_bdev,
    ctx: *mut c_void,
) {
    let handler = unsafe { &*(ctx as *mut T) };
    let device = unsafe { Device::with_owner(handler, bdev) };

    handler.handle_event(event.into(), &device);
}

/// Open a block device by its name.
pub(crate) fn open_desc(name: &CStr, write: bool) -> Result<NonNull<spdk_bdev_desc>> {
    let mut raw_desc = null_mut();

    to_result! {
        unsafe {
            spdk_bdev_open_ext(
                name.as_ptr(),
                write,
                Some(handle_desc_event::<()>),
                null_mut(),
                &mut raw_desc as *mut *mut _,
            )
        }
    }?;

    // SAFETY: If `spdk_bdev_open_ext` succeeds, `raw_desc` is a non-null and valid `spdk_bdev_desc`
    // pointer.
    Ok(unsafe { NonNull::new_unchecked(raw_desc) })
}

/// A handle to an open block device.
///
/// # Note
///
/// While it is safe to share a reference to a [`Descriptor`] across threads, the underlying
/// `spdk_bdev_desc` must be closed on the same `spdk_thread` on which it was opened. It is
/// therefore not marked as `Send`, though it is `Sync`.
#[derive(Debug)]
#[repr(transparent)]
pub struct Descriptor<'a, E: EventHandler>(NonNull<spdk_bdev_desc>, PhantomData<&'a E>);

unsafe impl<'a, E: EventHandler> Sync for Descriptor<'a, E> {}

impl<'a, E> Descriptor<'a, E>
where
    E: EventHandler,
{
    /// Open a block device by its name with a custom event handler.
    pub fn open_with_handler<R>(name: &CStr, write: bool, handler: R) -> Result<Self>
    where
        R: AsRef<E>,
        E: EventHandler,
    {
        let mut raw_desc = null_mut();

        to_result! {
            unsafe {
                spdk_bdev_open_ext(
                    name.as_ptr(),
                    write,
                    Some(handle_desc_event::<E>),
                    handler.as_ref() as *const _ as *mut _,
                    &mut raw_desc as *mut *mut _,
                )
            }
        }?;

        // SAFETY: If `spdk_bdev_open_ext` succeeds, `raw_desc` is a non-null and valid `spdk_bdev_desc`
        // pointer. This type is also a transparent wrapper around `NonNull<spdk_bdev_desc>`.
        Ok(unsafe { transmute::<NonNull<spdk_bdev_desc>, Self>(NonNull::new_unchecked(raw_desc)) })
    }

    /// Claims the block device of the [`Descriptor`] with the specified claim type and module.
    ///
    /// If the claim type is [`ReadManyWriteNone`], the descriptor must be read-only. Othwerise,
    /// the descriptor will be promoted to read/write if necessary.
    ///
    /// [`ReadManyWriteNone`]: ClaimType::ReadManyWriteNone
    #[cfg(feature = "bdev-module")]
    pub fn claim<M>(&self, type_: ClaimType<'_>, module: &Module<M>) -> Result<()>
    where
        M: ModuleInstance<M> + ModuleOps + 'static,
    {
        let (claim_type, mut opts) = type_.into_params()?;

        unsafe {
            to_result!(spdk_bdev_module_claim_bdev_desc(
                self.as_ptr(),
                claim_type,
                &mut opts as *mut _,
                module.as_ptr()
            ))
        }?;

        Ok(())
    }

    /// Returns a pointer to the underlying `spdk_bdev_desc` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev_desc {
        self.0.as_ptr()
    }

    /// Returns the [`Device`] associated with this [`Descriptor`].
    pub fn device(&self) -> Device<OwnedBy<'_, Self>> {
        unsafe { Device::with_owner(self, spdk_bdev_desc_get_bdev(self.0.as_ptr())) }
    }

    /// Returns an [`IoChannel`] for this [`Descriptor`].
    ///
    /// I/O channels are bound to the `spdk_thread` on which this function is called. The returned
    /// [`IoChannel`] cannot be used from any other thread.
    pub fn io_channel(&self) -> Result<IoChannel> {
        Ok(IoChannel(unsafe {
            NonNull::new(spdk_bdev_get_io_channel(self.0.as_ptr())).ok_or(ENOMEM)?
        }))
    }
}

impl Descriptor<'static, ()> {
    /// Open a block device by its name.
    pub fn open(name: &CStr, write: bool) -> Result<Self> {
        let desc = open_desc(name, write)?;

        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_bdev_desc>`.
        Ok(unsafe { transmute::<NonNull<spdk_bdev_desc>, Self>(desc) })
    }
}

impl<'a, E> Drop for Descriptor<'a, E>
where
    E: EventHandler,
{
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0.as_ptr()) }
    }
}
