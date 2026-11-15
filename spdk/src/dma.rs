//! Support for Storage Performance Development Kit DMA memory management.
//!
//! This module implements standard Rust memory allocation interfaces using the
//! SPDK memory allocator. The [`Buffer`] object manages a buffer allocated
//! using the SPDK memory allocator that is suitable for use with device I/O.
//!
//! See [Direct Memory Access (DMA) From User
//! Space](https://spdk.io/doc/memory.html) for more information on the SPDK
//! memory management.
use std::{
    alloc::{Allocator, Layout},
    cmp,
    io::{Cursor, IoSlice, IoSliceMut},
    ptr::{NonNull, copy_nonoverlapping, write_bytes},
    slice,
};

use spdk_sys::iovec as IoVec;

use crate::{
    Result,
    env::{CpuCore, NumaNode},
    errors::{EINVAL, ENOMEM},
};

/// A pointer type that uniquely owns a buffer allocated using the [`DmaAllocator`] memory allocator.
///
/// Its binary layout is equivalent to an `iovec` structure and can be used anywhere one is required.
///
/// [`DmaAllocator`]: crate::alloc::DmaAllocator
#[repr(transparent)]
pub struct Buffer(IoVec);

unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    /// Allocates a new [`Buffer] suitable for use with DMA I/O operations on the current NUMA node.
    pub fn new(layout: Layout) -> Result<Self> {
        Self::new_in(layout, CpuCore::current().numa_node())
    }

    /// Allocates a new [`Buffer] suitable for use with DMA I/O operations on the specified NUMA
    /// node.
    pub fn new_in(layout: Layout, numa_node: NumaNode) -> Result<Self> {
        let alloc = numa_node.dma_allocator().allocate(layout).or(Err(ENOMEM))?;

        Ok(Self(IoVec {
            iov_base: alloc.as_ptr().cast(),
            iov_len: alloc.len(),
        }))
    }

    /// Allocates a new [`Buffer] that has its memory zeroed and suitable for use with DMA I/O
    /// operations on the current NUMA node.
    pub fn new_zeroed(layout: Layout) -> Result<Self> {
        Self::new_zeroed_in(layout, CpuCore::current().numa_node())
    }

    /// Allocates a new [`Buffer] that has its memory zeroed and suitable for use with DMA I/O
    /// operations on the specified NUMA node.
    pub fn new_zeroed_in(layout: Layout, numa_node: NumaNode) -> Result<Self> {
        let alloc = numa_node
            .dma_allocator()
            .allocate_zeroed(layout)
            .or(Err(ENOMEM))?;

        Ok(Self(IoVec {
            iov_base: alloc.as_ptr().cast(),
            iov_len: alloc.len(),
        }))
    }

    /// Get a pointer to the buffer.
    pub fn as_ptr(&self) -> *const u8 {
        self.0.iov_base.cast()
    }

    /// Get a mutable pointer to the buffer.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.iov_base.cast()
    }

    /// Get the size of the buffer.
    pub fn size(&self) -> usize {
        self.0.iov_len
    }

    /// Get a slice referencing the buffer.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0.iov_base.cast(), self.0.iov_len) }
    }

    /// Get a mutable slice referencing the buffer.
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.0.iov_base.cast(), self.0.iov_len) }
    }

    /// Resize the buffer.
    pub fn resize(&mut self, new_layout: Layout) -> Result<()> {
        let new_alloc = unsafe {
            NumaNode::any()
                .dma_allocator()
                .reallocate(NonNull::new_unchecked(self.0.iov_base.cast()), new_layout)
                .or(Err(ENOMEM))?
        };

        self.0.iov_base = new_alloc.as_ptr().cast();
        self.0.iov_len = new_alloc.len();

        Ok(())
    }

    /// Sets the bytes of the buffer to zeroes.
    pub fn clear(&mut self) {
        unsafe {
            write_bytes(self.as_mut_ptr(), 0, self.size());
        }
    }

    /// Gets a read-only [`Cursor`] for the buffer.
    pub fn cursor(&self) -> Cursor<&[u8]> {
        Cursor::new(self.as_ref())
    }

    /// Gets a writable [`Cursor`] for the buffer.
    pub fn cursor_mut(&mut self) -> Cursor<&mut [u8]> {
        Cursor::new(self.as_mut())
    }

    /// Reads the number of bytes from the given offset.
    ///
    /// The offset is relative to the start of the buffer.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes written which may be less that the size of
    /// the source data.
    ///
    /// Returns [`EINVAL`] if the offset is greater than the
    /// size of the buffer.
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
        let size = self.size() as u64;

        if offset > size {
            return Err(EINVAL);
        }

        let bytes_to_copy = cmp::min(buf.len() as u64, size - offset) as usize;

        if bytes_to_copy != 0 {
            unsafe {
                copy_nonoverlapping(
                    self.as_ptr().add(offset as usize),
                    buf.as_mut_ptr(),
                    bytes_to_copy,
                );
            }
        }

        Ok(bytes_to_copy)
    }

    /// Writes a number of bytes starting from a given offset.
    ///
    /// The offset is relative to the start of the buffer. The buffer is not
    /// resized if there is an attempt to write past the end of the buffer.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes written which may be less that the size of
    /// the source data.
    ///
    /// Returns [`EINVAL`] if the offset is greater than the
    /// size of the buffer.
    pub fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize> {
        let size = self.size() as u64;

        if offset > size {
            return Err(EINVAL);
        }

        let bytes_to_copy = cmp::min(buf.len() as u64, size - offset) as usize;

        if bytes_to_copy != 0 {
            unsafe {
                copy_nonoverlapping(
                    buf.as_ptr(),
                    self.as_mut_ptr().add(offset as usize),
                    bytes_to_copy,
                );
            }
        }

        Ok(bytes_to_copy)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let ptr = unsafe { NonNull::new_unchecked(self.as_mut_ptr()) };
        let layout = unsafe { Layout::for_value_raw(ptr.as_ptr()) };

        unsafe { NumaNode::any().dma_allocator().deallocate(ptr, layout) };
    }
}

impl AsRef<Buffer> for Buffer {
    fn as_ref(&self) -> &Buffer {
        self
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<Buffer> for Buffer {
    fn as_mut(&mut self) -> &mut Buffer {
        self
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

impl<'a> AsRef<IoSlice<'a>> for Buffer {
    fn as_ref(&self) -> &IoSlice<'a> {
        unsafe { &*(&self.0 as *const _ as *const IoSlice<'a>) }
    }
}

impl<'a> AsMut<IoSliceMut<'a>> for Buffer {
    fn as_mut(&mut self) -> &mut IoSliceMut<'a> {
        unsafe { &mut *(&mut self.0 as *mut _ as *mut IoSliceMut<'a>) }
    }
}
