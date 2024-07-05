//! Support for pooled object allocation.
use std::{
    ffi::{
        c_void,
        CStr,
    }, marker::PhantomData,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    ptr::{
        self,
        NonNull,
    }
};

use spdk_sys::{
    spdk_mempool,

    SPDK_ENV_SOCKET_ID_ANY,

    spdk_mempool_count,
    spdk_mempool_create_ctor,
    spdk_mempool_free,
    spdk_mempool_from_obj,
    spdk_mempool_get,
    spdk_mempool_get_name,
    spdk_mempool_obj_iter,
    spdk_mempool_put,
};

/// Represents a pool of objects in their default state.
/// 
/// Use the [`Pool<T>::get()`] method to retreive an object from the pool as a
/// [`Pooled`] smart pointer type. The object will be re-assigned to its default
/// value and returned to the pool when the smart pointer is dropped.
/// 
/// # Notes
/// 
/// The object are stored with the default value in the pool. It is best to
/// avoid default values with memory or other resource allocations because these
/// allocations will live for the lifetime of the pool.
/// 
/// [`Pool<T>::get()`]: method@Pool<T>::get
pub struct Pool<T>(NonNull<spdk_mempool>, PhantomData<T>)
where
    T: Default;

unsafe impl Send for Pool<()> {}
unsafe impl Sync for Pool<()> {}

impl <T> Pool<T>
where
    T: Default
{
    unsafe extern "C" fn initialize_object(_mempool: *mut spdk_mempool, _ctx: *mut c_void, obj: *mut c_void, _obj_idx: u32) {
        let obj = obj as *mut T;
        ptr::write(obj, T::default());
    }

    /// Creates a new pool of objects.
    ///
    /// # Arguments
    ///
    /// `name`: The name of the pool.
    ///
    /// `count`: The number of objects in the pool.
    ///
    /// `cache_count`: The number of objects that may be cached in per-core
    /// caches. Use `SPDK_MEMPOOL_DEFAULT_CACHE_SIZE` for a reasonable default,
    /// or `0` for no per-core cache.
    pub fn new(name: &CStr, count: usize, cache_count: usize) -> Option<Self> {
        if let Some(pool) = NonNull::new(unsafe {
            spdk_mempool_create_ctor(
                name.as_ptr(),
                count,
                std::mem::size_of::<T>(),
                cache_count,
                SPDK_ENV_SOCKET_ID_ANY,
                Some(Self::initialize_object),
                ptr::null_mut(),
            )
        }) {
            return Some(Self(pool, PhantomData));
        }

        None
    }

    /// Returns the name of the pool.
    pub fn name(&self) -> &CStr {
        unsafe {
            CStr::from_ptr(spdk_mempool_get_name(self.0.as_ptr()))
        }
    }

    /// Returns the number of available objects in the pool.
    /// 
    /// # Notes
    /// 
    /// When the per-core caches are enabled, this method has to browse the
    /// length of all cores. It should not be used in performance-critical code
    /// but only for debugging purposes.
    pub fn available(&self) -> usize {
        unsafe {
            spdk_mempool_count(self.0.as_ptr())
        }
    }

    /// Gets an object from the pool.
    pub fn get(&self) -> Option<Pooled<T>> {
        NonNull::new(unsafe { spdk_mempool_get(self.0.as_ptr()) as *mut T })
            .map(Pooled)
    }

    /// Gets an object from the pool as a pinned smart pointer.
    pub fn get_pinned(&self) -> Option<Pin<Pooled<T>>> {
        self.get().map(|pooled| unsafe { Pin::new_unchecked(pooled) })
    }
}

impl <T> Drop for Pool<T>
where
    T: Default
{
    fn drop(&mut self) {
        unsafe {
            spdk_mempool_free(self.0.as_ptr());
        }
    }
}

impl <T> Pool<T>
where
    T: Default
{
    /// Invokes a function on each object in the pool.
    /// 
    /// # Return
    /// 
    /// This method returns the number of objects iterated.
    /// 
    /// # Safety
    /// 
    /// The caller must ensure that accessing the objects in the function is thread-safe.
    pub unsafe fn for_each<F>(&self, f: F) -> u32
    where
        F: FnMut(&T)
    {
        unsafe extern "C" fn callback<T, F>(_mp: *mut spdk_mempool, f: *mut c_void, obj: *mut c_void, _obj_idx: u32)
        where
            F: FnMut(&T)
        {
            let obj = obj as *const T;
            let f = &mut *(f as *mut F);

            f(&*obj);
        }

        let mut for_each_fn = Box::new(f);

        unsafe {
            spdk_mempool_obj_iter(self.0.as_ptr(), Some(callback::<T, F>), for_each_fn.as_mut() as *mut _ as *mut c_void)
        }
    }
}

/// A pointer type that uniquely owns an object allocated from a [`Pool<T>`].
pub struct Pooled<T: Default>(NonNull<T>);

impl <T> Drop for Pooled<T>
where
    T: Default
{
    fn drop(&mut self) {
        unsafe {
            *self.0.as_mut() = Default::default();

            spdk_mempool_put(spdk_mempool_from_obj(self.0.as_ptr().cast()), self.0.as_ptr().cast());
        }
    }
}

impl <T> AsRef<T> for Pooled<T>
where
    T: Default + ?Sized
{
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl <T> AsMut<T> for Pooled<T>
where
    T: Default + ?Sized
{
    fn as_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

impl <T> Deref for Pooled<T>
where
    T: Default + ?Sized
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl <T> DerefMut for Pooled<T>
where
    T: Default + ?Sized
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}

impl <T> Unpin for Pooled<T>
where
    T: Default + ?Sized
{}
