//! Support for pooled memory allocation.
use std::{
    alloc::{AllocError, Allocator, Layout},
    ffi::CStr,
    mem::{MaybeUninit, transmute},
    os::raw::c_void,
    ptr::{NonNull, write_bytes},
};

use spdk_sys::{
    SPDK_MAX_MEMPOOL_NAME_LEN, SPDK_MEMPOOL_ALIGN, SPDK_MEMPOOL_DEFAULT_CACHE_SIZE, spdk_mempool,
    spdk_mempool_count, spdk_mempool_create_ext, spdk_mempool_free, spdk_mempool_from_obj,
    spdk_mempool_get, spdk_mempool_get_name, spdk_mempool_get_priv, spdk_mempool_opts_init,
    spdk_mempool_put,
};

use crate::{
    Result,
    env::{CpuCore, NumaNode},
    errors::{EINVAL, ENOMEM},
};

type AllocResult = std::result::Result<NonNull<[u8]>, AllocError>;

struct MemPoolPrivate {
    elt_layout: Layout,
}

/// A thread-safe memory pool of fixed-sized allocations.
#[repr(transparent)]
pub struct MemPool {
    pool: NonNull<spdk_mempool>,
}

unsafe impl Send for MemPool {}
unsafe impl Sync for MemPool {}

impl MemPool {
    /// A callback function to initialize the private data associated with this mempool.
    unsafe extern "C" fn priv_init_cb(_mp: *mut spdk_mempool, ctx: *mut c_void, data: *mut c_void) {
        let elt_layout = unsafe { &*(ctx as *const Layout) };
        let mempool_priv = unsafe { &mut *(data as *mut MaybeUninit<MemPoolPrivate>) };

        mempool_priv.write(MemPoolPrivate {
            elt_layout: *elt_layout,
        });
    }

    /// Creates a new memory pool in the current NUMA node of `count` allocations with each having
    /// the layout described by `elt_layout`.
    pub fn new(name: &CStr, count: usize, elt_layout: Layout) -> Result<Self> {
        Self::new_in(name, count, elt_layout, CpuCore::current().numa_node())
    }

    /// Creates a new memory pool in the specified NUMA node of `count` allocations with each having
    /// the layout described by `elt_layout`.
    pub fn new_in(
        name: &CStr,
        count: usize,
        elt_layout: Layout,
        numa_node: NumaNode,
    ) -> Result<Self> {
        if name.count_bytes() > SPDK_MAX_MEMPOOL_NAME_LEN {
            return Err(EINVAL);
        }

        // Create a `Layout` satisfying the requested elements' layout and the mempool allocator's
        // aligment guarantees.
        let elt_layout = elt_layout
            .align_to(SPDK_MEMPOOL_ALIGN)
            .or(Err(EINVAL))?
            .pad_to_align();

        let mut opts = MaybeUninit::uninit();

        unsafe {
            spdk_mempool_opts_init(opts.as_mut_ptr());
        }

        let mut opts = unsafe { opts.assume_init() };

        opts.priv_size = size_of::<MemPoolPrivate>();
        opts.priv_init = Some(Self::priv_init_cb);
        opts.priv_init_arg = &elt_layout as *const _ as *mut _;

        NonNull::new(unsafe {
            spdk_mempool_create_ext(
                name.as_ptr(),
                count,
                elt_layout.size(),
                SPDK_MEMPOOL_DEFAULT_CACHE_SIZE,
                numa_node.id(),
                &opts as *const _ as *mut _,
            )
        })
        .map(|pool| unsafe { transmute(pool) })
        .ok_or(ENOMEM)
    }

    /// Returns the name of the memory pool.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_mempool_get_name(self.pool.as_ptr())) }
    }

    /// Returns a reference to the mempool's private data.
    fn private_data(&self) -> &MemPoolPrivate {
        unsafe { &*(spdk_mempool_get_priv(self.pool.as_ptr()) as *const MemPoolPrivate) }
    }

    /// Returns the [`Layout`] of each allocation in the memory pool.
    pub fn element_layout(&self) -> &Layout {
        &self.private_data().elt_layout
    }

    /// Returns the number of allocations in the memory pool that are currently
    /// available.
    pub fn available(&self) -> usize {
        unsafe { spdk_mempool_count(self.pool.as_ptr()) }
    }
}

unsafe impl Allocator for MemPool {
    fn allocate(&self, layout: Layout) -> AllocResult {
        let elt_layout = self.element_layout();
        let aligned_layout = layout.align_to(elt_layout.align()).or(Err(AllocError))?;

        if aligned_layout.size() > elt_layout.size() {
            return Err(AllocError);
        }

        NonNull::new(unsafe { spdk_mempool_get(self.pool.as_ptr()) as *mut u8 })
            .map(|ptr| NonNull::slice_from_raw_parts(ptr, elt_layout.size()))
            .ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _layout: Layout) {
        let raw_ptr = ptr.as_ptr().cast();

        assert!(unsafe { spdk_mempool_from_obj(raw_ptr) == self.pool.as_ptr() });

        unsafe { spdk_mempool_put(self.pool.as_ptr(), raw_ptr) };
    }

    unsafe fn grow(&self, ptr: NonNull<u8>, old_layout: Layout, new_layout: Layout) -> AllocResult {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        if new_layout.size() >= self.private_data().elt_layout.size() {
            return Err(AllocError);
        }

        Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> AllocResult {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        if new_layout.size() >= self.private_data().elt_layout.size() {
            return Err(AllocError);
        }

        unsafe {
            write_bytes(
                ptr.as_ptr().add(old_layout.size()),
                0,
                new_layout.size() - old_layout.size(),
            )
        };

        Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> AllocResult {
        debug_assert!(
            new_layout.size() <= old_layout.size(),
            "`new_layout.size()` must be less than or equal to `old_layout.size()`"
        );

        debug_assert!(
            new_layout.size() <= self.private_data().elt_layout.size(),
            "`new_layout.size()` must be less than or equal to `elt_layout.size()`"
        );

        Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
    }
}

impl Drop for MemPool {
    fn drop(&mut self) {
        unsafe {
            spdk_mempool_free(self.pool.as_ptr());
        }
    }
}
