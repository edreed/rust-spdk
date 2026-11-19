#![cfg(feature = "bdev-module")]
use std::{
    ffi::CStr,
    io::{Cursor, Write},
    mem::{MaybeUninit, size_of},
    slice,
};

use libc::EINVAL;
use spdk_sys::{
    SPDK_BDEV_CLAIM_READ_MANY_WRITE_NONE, SPDK_BDEV_CLAIM_READ_MANY_WRITE_ONE,
    SPDK_BDEV_CLAIM_READ_MANY_WRITE_SHARED, spdk_bdev_claim_opts, spdk_bdev_claim_opts_init,
    spdk_bdev_claim_type,
};

use crate::Result;

/// Represents the type of claim on a block device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimType<'a> {
    /// Exclusive write claim. Only the claimant can have write access to the block device.
    ///
    /// The `String` parameter represents the name of the claimant and is used for logging purposes.
    /// It must be 31 characters or less.
    ReadManyWriteOne(&'a CStr),

    /// Read many, write none claim. Multiple entities can have read access, but no entity can have
    /// write access to the block device.
    ///
    /// The `String` parameter represents the name of the claimant and is used for logging purposes.
    /// It must be 31 characters or less.
    ReadManyWriteNone(&'a CStr),

    /// Read many, write many shared claim. Multiple entities using the same shared claim key can
    /// have read and write access to the block device.
    ///
    /// The `String` parameter represents the name of the claimant and is used for logging purposes.
    /// It must be 31 characters or less.
    ///
    /// The `u64` parameter represents the shared claim key.
    ReadManyWriteShared(&'a CStr, u64),
}

impl<'a> ClaimType<'a> {
    /// Converts the `ClaimType` into the corresponding SPDK claim type and claim options.
    pub(crate) fn into_params(self) -> Result<(spdk_bdev_claim_type, spdk_bdev_claim_opts)> {
        let (claim_type, name, shared_claim_key) = match self {
            ClaimType::ReadManyWriteOne(name) => (SPDK_BDEV_CLAIM_READ_MANY_WRITE_ONE, name, 0),
            ClaimType::ReadManyWriteNone(name) => (SPDK_BDEV_CLAIM_READ_MANY_WRITE_NONE, name, 0),
            ClaimType::ReadManyWriteShared(name, key) => {
                (SPDK_BDEV_CLAIM_READ_MANY_WRITE_SHARED, name, key)
            }
        };

        let mut opts = MaybeUninit::uninit();

        unsafe {
            spdk_bdev_claim_opts_init(opts.as_mut_ptr(), size_of::<spdk_bdev_claim_opts>());
        }

        let mut opts = unsafe { opts.assume_init() };

        let mut claim_name = Cursor::new(unsafe {
            slice::from_raw_parts_mut(opts.name.as_mut_ptr() as *mut u8, opts.name.len())
        });

        claim_name
            .write_all(name.to_bytes_with_nul())
            .map_err(|_| EINVAL)?;

        opts.shared_claim_key = shared_claim_key;

        Ok((claim_type, opts))
    }
}
