//! Support for pooled memory allocation.
use std::{
    ffi::CStr,
    mem::{
        self,

        ManuallyDrop,
        MaybeUninit,

        offset_of,

        size_of_val,
    },
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    process::abort,
    ptr::{
        self,
        
        NonNull,

        addr_of_mut,

        drop_in_place,
    },
    sync::atomic::{
        AtomicUsize,
        Ordering,
    }
};

use spdk_sys::{
    spdk_mempool,

    SPDK_ENV_SOCKET_ID_ANY,
    SPDK_MEMPOOL_DEFAULT_CACHE_SIZE,

    spdk_mempool_count,
    spdk_mempool_create,
    spdk_mempool_free,
    spdk_mempool_from_obj,
    spdk_mempool_get,
    spdk_mempool_get_name,
    spdk_mempool_put,
};

use crate::errors::{
    Errno,

    EINVAL,
    ENOMEM,
};

/// A thread-safe memory pool of fixed-sized allocations.
pub struct MemPool{
    pool: NonNull<spdk_mempool>,
    alloc_size: usize,
}

unsafe impl Send for MemPool {}
unsafe impl Sync for MemPool {}

impl MemPool {
    /// Creates a new memory pool of `count` allocations, each of size `alloc_size` bytes.
    pub fn new(name: &CStr, count: usize, alloc_size: usize) -> Option<Self> {
        NonNull::new(unsafe {
            spdk_mempool_create(
                name.as_ptr(),
                count,
                alloc_size,
                SPDK_MEMPOOL_DEFAULT_CACHE_SIZE,
                SPDK_ENV_SOCKET_ID_ANY,
            )
        })
        .map(|pool| Self { pool, alloc_size })
    }

    /// Returns the name of the memory pool.
    pub fn name(&self) -> &CStr {
        unsafe {
            CStr::from_ptr(spdk_mempool_get_name(self.pool.as_ptr()))
        }
    }

    /// Returns the size, in bytes, of each allocation in the memory pool.
    pub fn allocation_size(&self) -> usize {
        self.alloc_size
    }

    /// Returns the number of allocations in the memory pool that are currently
    /// available.
    pub fn available(&self) -> usize {
        unsafe {
            spdk_mempool_count(self.pool.as_ptr())
        }
    }

    /// Gets an allocation from the memory pool and places `val` into it..
    /// 
    /// The allocation is owned by the returned [`Pooled<T>`] smart pointer
    /// object. Like, [`Box<T>`], the allocation's value will be dropped and the
    /// allocation returned to its memory pool when the `Pooled` object is
    /// dropped.
    /// 
    /// The size of the value type must be less than or equal to the allocation
    /// size of the memory pool. Otherwise, this method returns `Err(EINVAL)`.
    /// 
    /// If there are no available allocations in the memory pool, this returns
    /// `Err(ENOMEM)`.
    pub fn get_with<T>(&self, val: T) -> Result<Pooled<T>, Errno> {
        if size_of_val(&val) > self.alloc_size {
            return Err(EINVAL);
        }

        NonNull::new(unsafe { spdk_mempool_get(self.pool.as_ptr()) as *mut u8 })
            .map(|o| {
                unsafe {
                    ptr::write(o.as_ptr() as *mut T, val);
                }

                NonNull::cast::<T>(o)
            })
            .map(Pooled)
            .ok_or(ENOMEM)
    }

    /// Gets an allocation from the memory pool and places `T`'s default value
    /// into it.
    /// 
    /// The allocation is owned by the returned [`Pooled<T>`] smart pointer
    /// object. Like, [`Box<T>`], the allocation's value will be dropped and the
    /// allocation returned to its memory pool when the `Pooled` object is
    /// dropped.
    /// 
    /// The size of the value type must be less than or equal to the allocation
    /// size of the memory pool. Otherwise, this method returns `Err(EINVAL)`.
    /// 
    /// If there are no available allocations in the memory pool, this returns
    /// `Err(ENOMEM)`.
    pub fn get<T: Default>(&self) -> Result<Pooled<T>, Errno> {
        self.get_with(T::default())
    }
}

impl Drop for MemPool {
    fn drop(&mut self) {
        unsafe {
            spdk_mempool_free(self.pool.as_ptr());
        }
    }
}

/// A pointer type that uniquely owns an object allocated from a [`MemPool`].
pub struct Pooled<T: ?Sized>(NonNull<T>);

unsafe impl <T: ?Sized + Send> Send for Pooled<T> {}
unsafe impl <T: ?Sized + Sync> Sync for Pooled<T> {}

impl <T> Pooled<T> {
    /// Gets an allocation from the memory pool and places `val` into it.
    pub fn try_new(val: T, pool: &MemPool) -> Result<Self, Errno> {
        pool.get_with(val)
    }
    /// Converts the [`Pooled<T>`] object into a [`Pin<Pooled<T>>`] object. If
    /// `T` does not implement [`Unpin`], then `*pooled` will be pinned in
    /// memory and unable to be moved.
    /// 
    /// This method has the same behavior and characteristics as the
    /// [`Box<T>::into_pin()`] method.
    /// 
    /// [`Box<T>::into_pin()`]: fn@std::boxed::Box::into_pin
    pub fn into_pin(pooled: Self) -> Pin<Self> {
        // SAFETY: Like `Box<T>`, it is not possible to move or replace the
        // insides of a `Pin<Pooled<T>>` when `T: !Unpin`. It is safe to pin
        // it directly without an additional requirements.
        unsafe { Pin::new_unchecked(pooled) }
    }
}

impl <T: ?Sized> Pooled<T> {
    /// Converts the [`Pooled<T>`] object into a raw pointer.
    /// 
    /// After calling this function, the caller is responsible for the memory
    /// previously managed by the `Pooled` object. In order to properly destroy
    /// `T` and return it to the memory pool, the caller must convert the raw
    /// pointer back into a `Pooled` object using the [`Pooled::from_raw()`]
    /// function. This allows the `Pooled` destructor to properly clean up.
    /// 
    /// [`Pooled::from_raw()`]: fn@Pooled::from_raw
    pub fn into_raw(pooled: Self) -> *mut T {
        ManuallyDrop::new(pooled).0.as_ptr()
    }

    /// Converts a raw pointer into a [`Pooled<T>`] object.
    /// 
    /// After calling this function, the raw pointer is owned by the returned
    /// `Pooled` object. The `Pooled` destructor will call the destructor of `T`
    /// and return it's memory to the memory pool.
    /// 
    /// # Safety
    /// 
    /// The caller must ensure that the raw pointer was previously allocated by
    /// the [`MemPool`] that created the `Pooled` object.
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self(unsafe { NonNull::new_unchecked(ptr) })
    }
}

impl <T: ?Sized> Drop for Pooled<T> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(self.0.as_ptr());

            let ptr = self.0.as_ptr().cast();

            spdk_mempool_put(spdk_mempool_from_obj(ptr), ptr);
        }
    }
}

impl <T: ?Sized> AsRef<T> for Pooled<T> {
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl <T: ?Sized> AsMut<T> for Pooled<T> {
    fn as_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

impl <T: ?Sized> Deref for Pooled<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl <T: ?Sized> DerefMut for Pooled<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}

impl <T: ?Sized> Unpin for Pooled<T> {}

impl <T> From<Pooled<T>> for Pin<Pooled<T>> {
    fn from(pooled: Pooled<T>) -> Self {
        Pooled::into_pin(pooled)
    }
}

const MAX_REFCOUNT: usize = (isize::MAX) as usize;

#[repr(C)]
struct ArcPooledInner<T: ?Sized> {
    strong: AtomicUsize,
    weak: AtomicUsize,
    data: T,
}

/// A thread-safe reference-counted pointer type that owns an object allocated
/// from a [`MemPool`].
pub struct ArcPooled<T: ?Sized> {
    inner: NonNull<ArcPooledInner<T>>,
}

unsafe impl <T: ?Sized + Send + Sync> Send for ArcPooled<T> {}
unsafe impl <T: ?Sized + Sync + Sync> Sync for ArcPooled<T> {}

impl <T> ArcPooled<T> {
    /// Gets an allocation from the memory pool and places `val` into an
    /// [`ArcPooled<T>`] object.
    pub fn try_new(data: T, pool: &MemPool) -> Result<Self, Errno> {
        let inner = ArcPooledInner {
            strong: AtomicUsize::new(1),
            weak: AtomicUsize::new(1),
            data,
        };

        let inner = pool.get_with(inner)?;

        Ok(Self { inner: unsafe { NonNull::new_unchecked(Pooled::into_raw(inner))} })
    }

    /// Gets an allocation from the memory pool and constructs a new
    /// [`ArcPooled<T>`] while giving you a [`WeakPooled<T>`] to the same
    /// allocation. This allows you to construct `T` which holds a weak pointer
    /// to itself.
    /// 
    /// This function has the same behavior and characteristics as the
    /// [`Arc::new_cyclic()`] function.
    /// 
    /// [`Arc::new_cyclic()`]: fn@std::sync::Arc::new_cyclic
    pub fn try_new_cyclic<F>(pool: &MemPool, data_fn: F) -> Result<Self, Errno>
    where
        F: FnOnce(&WeakPooled<T>) -> T
    {
        let uninit = pool.get_with(ArcPooledInner {
            strong: AtomicUsize::new(0),
            weak: AtomicUsize::new(1),
            data: MaybeUninit::<T>::uninit(),
        })?;

        let mut inner: NonNull<ArcPooledInner<T>> = unsafe {
            NonNull::new_unchecked(Pooled::into_raw(uninit)).cast()
        };

        let weak = WeakPooled { inner: inner.clone() };
        let data = data_fn(&weak);

        let strong: ArcPooled<T> = unsafe {
            ptr::write(addr_of_mut!(inner.as_mut().data), data);
            inner.as_ref().strong.store(1, Ordering::Release);
            Self { inner }
        };

        mem::forget(weak);

        Ok(strong)
    }

    /// Converts a raw pointer to an [`ArcPooled<T>`] object.
    /// 
    /// The pointer must previously have been obtained by calling the
    /// [`ArcPooled<U>::into_raw()`] function with the following requirements:
    /// 
    /// * If `U` is sized, it must have the same size and aligment as `T`. This
    ///   is trivially true if `U` is `T`.
    /// * If `U` is unsized, its data pointer must have the same size and
    ///   alignment as `T`. This is trvially tru if `ArcPooled<U>` was
    ///   contructed through `ArcPooled<T>` and the converted to `ArcPooled<U>`
    ///   through an [unsized coercion].
    /// 
    /// Note that if `U` or `U`'s data pointer is not `T` but has the same size
    /// and alignment, this is basically like transmuting references of
    /// different types. See [`mem::transmute`] for more information on what
    /// restrictions apply in this case.
    /// 
    /// The user of from_raw has to make sure a specific value of T is only
    /// dropped once.
    /// 
    /// This function is unsafe because improper use may lead to memory
    /// unsafety, even if the returned Arc<T> is never accessed.
    /// 
    /// [`ArcPooled<U>::into_raw()`]: fn@ArcPooled::into_raw
    /// [unsized coercion]: https://doc.rust-lang.org/reference/type-coercions.html#unsized-coercions
    /// [`mem::transmute`]: fn@std::mem::transmute
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        let inner_ptr = ptr.sub(offset_of!(ArcPooledInner<T>, data)) as *mut ArcPooledInner<T>;
        Self { inner: NonNull::new_unchecked(inner_ptr) }
    }
}

impl <T: ?Sized> ArcPooled<T> {
    /// Gets the number of strong (i.e. `ArcPooled`) pointers to this
    /// allocation.
    pub fn strong_count(this: &Self) -> usize {
        unsafe { this.inner.as_ref().strong.load(Ordering::Relaxed) }
    }

    /// Gets the number of weak (i.e. `WeakPooled`) pointers to this allocation.
    /// 
    /// Note that all strong pointers share an implicit weak reference. As long
    /// as there is at least one strong reference, the weak count will always be
    /// `>= 1`.
    pub fn weak_count(this: &Self) -> usize {
        unsafe { this.inner.as_ref().weak.load(Ordering::Relaxed) }
    }

    /// Creates a weak pointer to this allocation.
    pub fn downgrade(this: &Self) -> WeakPooled<T> {
        let old_count = unsafe { this.inner.as_ref().weak.fetch_add(1, Ordering::Relaxed) };

        if old_count >= MAX_REFCOUNT {
            abort();
        }

        WeakPooled { inner: this.inner.clone() }
    }

    /// Converts the [`ArcPooled<T>`] object into a raw pointer.
    /// 
    /// To avoid a memory leak, the returned pointer must be converted back into
    /// an `ArcPooled<T>` object using the [`ArcPooled::from_raw()`] function.
    /// 
    /// [`ArcPooled::from_raw()`]: fn@ArcPooled::from_raw
    pub fn into_raw(this: Self) -> *mut T {
        unsafe { addr_of_mut!(ManuallyDrop::new(this).inner.as_mut().data) }
    }
}

impl <T: ?Sized> Clone for ArcPooled<T> {
    fn clone(&self) -> Self {
        let old_count = unsafe { self.inner.as_ref().strong.fetch_add(1, Ordering::Relaxed) };

        if old_count >= MAX_REFCOUNT {
            abort();
        }

        Self { inner: self.inner.clone() }
    }
}

impl <T: ?Sized> Drop for ArcPooled<T> {
    fn drop(&mut self) {
        let old_count = unsafe { self.inner.as_ref().strong.fetch_sub(1, Ordering::AcqRel) };

        if old_count == 1 {
            unsafe { drop_in_place(&mut self.inner.as_mut().data); }

            WeakPooled{ inner: self.inner.clone() };
        } else if old_count >= MAX_REFCOUNT {
            abort();
        }
    }
}

impl <T: ?Sized> Deref for ArcPooled<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.inner.as_ref().data }
    }
}

impl <T: ?Sized> AsRef<T> for ArcPooled<T> {
    fn as_ref(&self) -> &T {
        unsafe { &self.inner.as_ref().data }
    }
}

/// A thread-safe weak reference to an object allocated from a [`MemPool`].
/// 
/// A weak reference is a non-owning reference to the managed allocation. The
/// allocation object is accessed by calling the [`upgrade()`] method on the
/// `WeakPooled` pointer. The `upgrade()` method returns an
/// `Option<ArcPooled<T>>` object.
/// 
/// Since a weak reference does not count towards ownership of the allocation,
/// it will not prevent the allocation from being dropped and no guarantee is
/// provided about the value still being present. The `upgrade()` method may
/// return `None` to incidate that the value is no longer present. However, a
/// weak reference does prevennt the allocation itself from being returned to
/// the `MemPool` from whence it came.
/// 
/// A weak reference is useful for keeping a temporary reference to the
/// allocation managed by [`ArcPooled<T>`] without preventing its inner value
/// from being dropped. It can also be used to prevent circular references
/// between `ArcPooled<T>` pointers.
/// 
/// To obtain a weak-reference, call the [`ArcPooled::downgrade()`] method.
/// 
/// [`upgrade()`]: fn@WeakPooled::upgrade
/// [`ArcPooled::downgrade()`]: fn@ArcPooled::downgrade
pub struct WeakPooled<T: ?Sized> {
    inner: NonNull<ArcPooledInner<T>>,
}

unsafe impl <T: ?Sized + Send + Sync> Send for WeakPooled<T> {}
unsafe impl <T: ?Sized + Sync + Sync> Sync for WeakPooled<T> {}

impl <T: ?Sized> WeakPooled<T> {
    /// Attempts to upgrade the weak reference to a strong reference.
    /// 
    /// This method returns `None` if the inner value has already been dropped.
    pub fn upgrade(&self) -> Option<ArcPooled<T>> {
        let inner = unsafe { self.inner.as_ref() };
        let mut strong_count = inner.strong.load(Ordering::Acquire);

        loop {
            if strong_count == 0 {
                return None;
            }

            match inner.strong.compare_exchange_weak(
                strong_count,
                strong_count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
                )
            {
                Ok(old_count) => {
                    if old_count >= MAX_REFCOUNT {
                        abort();
                    }

                    return Some(ArcPooled { inner: self.inner.clone() })
                },
                Err(count) => strong_count = count,
            }
        }
    }
}

impl <T: ?Sized> Clone for WeakPooled<T> {
    fn clone(&self) -> Self {
        let old_count = unsafe { self.inner.as_ref().weak.fetch_add(1, Ordering::Relaxed) };

        if old_count >= MAX_REFCOUNT {
            abort();
        }

        Self { inner: self.inner.clone() }
    }
}

impl <T: ?Sized> Drop for WeakPooled<T> {
    fn drop(&mut self) {
        let old_count = unsafe { self.inner.as_ref().weak.fetch_sub(1, Ordering::AcqRel) };

        if old_count == 1 {
            let ptr = self.inner.as_ptr().cast();

            unsafe { spdk_mempool_put(spdk_mempool_from_obj(ptr), ptr); }
        } else if old_count >= MAX_REFCOUNT {
            abort();
        }
    }
}
