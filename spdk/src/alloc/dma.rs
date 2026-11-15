use std::{
    alloc::{AllocError, Allocator, Layout},
    mem::transmute,
    ptr::{NonNull, null_mut},
};

use spdk_sys::{
    SPDK_CACHE_LINE_SIZE, spdk_dma_free, spdk_dma_malloc_socket, spdk_dma_realloc,
    spdk_dma_zmalloc_socket,
};

use crate::env::NumaNode;

use super::AllocResult;

/// An allocator that can provide pinned memory buffers suitable for use with DMA I/O operations in
/// user space.
///
/// An instance of this allocator may be obtained via the [`NumaNode::dma_allocator`] method. If the
/// [`NumaNode`] represents a specific NUMA node, the provided buffers will come from memory of the
/// node. If NUMA alignment is not necessary, `NumaNode::any().dma_allocator()` yields an instance
/// that can provide buffers from any node.
#[repr(transparent)]
pub struct DmaAllocator(NumaNode);

impl DmaAllocator {
    /// Resizes the pinned memory buffer previously allocated using [`DmaAllocator`] using the new
    /// [`Layout`] for size and alignment.
    ///
    /// The existing contents of the buffer are preserved.
    ///
    /// # Safety
    ///
    /// Callers of this method must meet the safety requirements of the [`Allocator::grow`] or
    /// [`Allocator::shrink`] methods.
    ///
    /// [`Layout`]: https://doc.rust-lang.org/stable/std/alloc/struct.Layout.html
    pub unsafe fn reallocate(
        &self,
        old_ptr: NonNull<u8>,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        let new_layout = new_layout
            .align_to(SPDK_CACHE_LINE_SIZE)
            .or(Err(AllocError))?;

        NonNull::new(unsafe {
            spdk_dma_realloc(
                old_ptr.as_ptr().cast(),
                new_layout.size(),
                new_layout.align(),
                null_mut(),
            ) as *mut u8
        })
        .map(|ptr| NonNull::slice_from_raw_parts(ptr, new_layout.size()))
        .ok_or(AllocError)
    }
}

unsafe impl Allocator for DmaAllocator {
    fn allocate(&self, layout: Layout) -> AllocResult {
        let new_layout = layout.align_to(SPDK_CACHE_LINE_SIZE).or(Err(AllocError))?;

        NonNull::new(unsafe {
            spdk_dma_malloc_socket(
                new_layout.size(),
                new_layout.align(),
                null_mut(),
                self.0.id(),
            ) as *mut u8
        })
        .map(|ptr| NonNull::slice_from_raw_parts(ptr, new_layout.size()))
        .ok_or(AllocError)
    }

    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let new_layout = layout.align_to(SPDK_CACHE_LINE_SIZE).or(Err(AllocError))?;

        NonNull::new(unsafe {
            spdk_dma_zmalloc_socket(
                new_layout.size(),
                new_layout.align(),
                null_mut(),
                self.0.id(),
            ) as *mut u8
        })
        .map(|ptr| NonNull::slice_from_raw_parts(ptr, new_layout.size()))
        .ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: std::ptr::NonNull<u8>, _layout: std::alloc::Layout) {
        let raw_ptr = ptr.as_ptr().cast();

        unsafe { spdk_dma_free(raw_ptr) };
    }

    unsafe fn grow(
        &self,
        old_ptr: NonNull<u8>,
        _old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        unsafe { Self::reallocate(self, old_ptr, new_layout) }
    }

    unsafe fn shrink(
        &self,
        old_ptr: NonNull<u8>,
        _old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        unsafe { Self::reallocate(self, old_ptr, new_layout) }
    }
}

impl From<NumaNode> for DmaAllocator {
    fn from(numa_node: NumaNode) -> Self {
        unsafe { transmute(numa_node) }
    }
}
