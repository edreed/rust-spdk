#![cfg_attr(docsrs, doc(cfg(feature = "bdev-module")))]

use std::{marker::PhantomData, mem::transmute, ptr::NonNull};

use spdk_sys::{spdk_io_channel, spdk_io_channel_get_ctx, spdk_io_channel_get_thread};

use crate::{
    Result,
    errors::ENOMEM,
    thread::{self, Thread},
};

/// A BDev I/O channel implementation.
///
/// The type parameter `T` is the I/O channel context type for the BDev implementation.
#[repr(transparent)]
pub struct IoChannelImpl<T> {
    channel: NonNull<spdk_io_channel>,
    _ctx: PhantomData<T>,
}

impl<T> IoChannelImpl<T> {
    /// Constructs a new I/O channel from a raw pointer.
    pub(super) fn from_raw(channel: *mut spdk_io_channel) -> Result<Self> {
        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_io_channel>`.
        NonNull::new(channel)
            .map(|p| unsafe { transmute(p) })
            .ok_or(ENOMEM)
    }

    /// Constructs a new I/O channel from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw pointer is non-null and valid.
    pub(super) unsafe fn from_raw_unchecked(channel: *mut spdk_io_channel) -> Self {
        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_io_channel>`.
        unsafe { transmute(NonNull::new_unchecked(channel)) }
    }

    /// Converts the I/O channel into a raw pointer.
    pub(super) fn into_raw(self) -> *mut spdk_io_channel {
        self.channel.as_ptr()
    }

    /// Returns a reference to the I/O channel context.
    pub fn ctx(&self) -> &T {
        unsafe { &*spdk_io_channel_get_ctx(self.channel.as_ptr()).cast() }
    }

    /// Returns a mutable reference to the I/O channel context.
    pub fn ctx_mut(&mut self) -> &mut T {
        unsafe { &mut *spdk_io_channel_get_ctx(self.channel.as_ptr()).cast() }
    }

    /// Returns the thread associated with the I/O channel.
    pub fn thread(&self) -> Thread<thread::OwnedBy<'_, Self>> {
        // SAFETY: The thread associated with the I/O channel is guaranteed to be non-null and
        // valid for the lifetime of the I/O channel.
        unsafe {
            Thread::with_owner_from_raw(self, spdk_io_channel_get_thread(self.channel.as_ptr()))
        }
    }
}
