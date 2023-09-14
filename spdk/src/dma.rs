//! Support for Storage Performance Development Kit DMA memory management.
use std::{
    alloc::{
        Layout,
        
        handle_alloc_error
    },
    cmp,
    io::Cursor,
    ptr::{
        copy_nonoverlapping,
        null_mut,
        write_bytes,
    },
    slice,
};

use spdk_sys::{
    Errno,

    spdk_dma_free,
    spdk_dma_malloc,
    spdk_dma_realloc,
    spdk_dma_zmalloc,
};

use crate::errors::EINVAL;

/// Allocates memory using the SPDK memory allocator.
pub fn alloc(layout: Layout) -> *mut u8 {
    let mem = unsafe {
        spdk_dma_malloc(layout.size(), layout.align(), null_mut()) as *mut u8
    };

    if mem.is_null() {
        handle_alloc_error(layout);
    }

    mem
}

/// Allocates zeroed memory using the SPDK memory allocator.
pub fn alloc_zeroed(layout: Layout) -> *mut u8 {
    let mem = unsafe {
        spdk_dma_zmalloc(layout.size(), layout.align(), null_mut()) as *mut u8
    };

    if mem.is_null() {
        handle_alloc_error(layout);
    }

    mem
}

/// Reallocates memory previously allocated using the SPDK memory allocator.
pub fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    let mem = unsafe {
        spdk_dma_realloc(ptr as *mut _, new_size, layout.align(), null_mut()) as *mut u8
    };

    if mem.is_null() {
        handle_alloc_error(layout);
    }

    mem
}

/// Frees memory previously allocated using the SPDK memory allocator.
pub fn free(ptr: *mut u8) {
    unsafe {
        spdk_dma_free(ptr as *mut _);
    }
}

/// A buffer allocated using the SPDK memory allocator.
pub struct Buffer {
    buf: *mut u8,
    layout: Layout,
}

unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    /// Allocates a new buffer using the SPDK memory allocator.
    pub fn new(layout: Layout) -> Self {
        Self {
            buf: alloc(layout),
            layout
        }
    }

    /// Allocates a new zeroed buffer using the SPDK memory allocator.
    pub fn new_zeroed(layout: Layout) -> Self {
        Self {
            buf: alloc_zeroed(layout),
            layout
        }
    }

    /// Get a pointer to the buffer.
    pub fn as_ptr(&self) -> *mut u8 {
        self.buf
    }

    /// Get a mutable pointer to the buffer.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.buf
    }

    /// Get the size of the buffer.
    pub fn size(&self) -> usize {
        self.layout.size()
    }

    /// Get a slice referencing the buffer.
    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self.buf, self.layout.size())
        }
    }

    /// Get a mutable slice referencing the buffer.
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self.buf, self.layout.size())
        }
    }

    /// Resize the buffer.
    pub fn resize(&mut self, new_size: usize) {
        self.buf = realloc(self.buf, self.layout, new_size);
        self.layout = Layout::from_size_align(new_size, self.layout.align()).unwrap();
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
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize, Errno> {
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
                    bytes_to_copy);
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
    pub fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, Errno> {
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
                    bytes_to_copy);
            }
        }

        Ok(bytes_to_copy)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        free(self.buf);
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
