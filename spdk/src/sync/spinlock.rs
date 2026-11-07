use std::sync::OnceLock;

use lock_api::{GuardNoSend, Mutex, MutexGuard, RawMutex};
use spdk_sys::{
    spdk_spin_destroy, spdk_spin_held, spdk_spin_init, spdk_spin_lock, spdk_spin_unlock,
    spdk_spinlock,
};

/// A raw spinlock implementation based on the SPDK's [`spdk_spinlock`].
///
/// [`spdk_spinlock`]: spdk_sys::spdk_spinlock
#[derive(Debug)]
pub struct RawSpinLock {
    spinlock: OnceLock<Box<spdk_spinlock>>,
}

impl RawSpinLock {
    /// Creates a new [`RawSpinLock`] instance.
    pub const fn new() -> Self {
        Self {
            spinlock: OnceLock::new(),
        }
    }

    /// Returns a raw pointer to the underlying `spdk_spinlock` structure.
    fn as_ptr(&self) -> *mut spdk_spinlock {
        self.spinlock
            .get_or_init(|| {
                let mut sspin = Box::new_uninit();

                unsafe {
                    spdk_spin_init(sspin.as_mut_ptr());
                    sspin.assume_init()
                }
            })
            .as_ref() as *const _ as *mut _
    }
}

impl Default for RawSpinLock {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RawSpinLock {
    fn drop(&mut self) {
        if let Some(sspin) = self.spinlock.get_mut() {
            unsafe {
                spdk_spin_destroy(sspin.as_mut() as *mut _);
            }
        }
    }
}

unsafe impl RawMutex for RawSpinLock {
    const INIT: Self = Self::new();

    type Guard = GuardNoSend;

    fn lock(&self) -> Self::Guard {
        unsafe {
            spdk_spin_lock(self.as_ptr());
        }

        GuardNoSend::new()
    }

    fn try_lock(&self) -> Option<Self::Guard> {
        unimplemented!()
    }

    unsafe fn unlock(&self, _guard: Self::Guard) {
        unsafe {
            spdk_spin_unlock(self.as_ptr());
        }
    }

    fn is_locked(&self) -> bool {
        unsafe { spdk_spin_held(self.as_ptr()) }
    }
}

/// A spinlock implementation based on the SPDK's [`spdk_spinlock`].
///
/// Like [`Mutex`], a `SpinLock` is a mutual exlusion primitive useful for protecting shared data.
///
/// [`Mutex`]: https://docs.rs/lock_api/latest/lock_api/struct.Mutex.html
/// [`spdk_spinlock`]: spdk_sys::spdk_spinlock
pub type SpinLock<T> = Mutex<RawSpinLock, T>;

/// An RAII guard which unlocks the spinlock when dropped.
pub type SpinLockGuard<'a, T> = MutexGuard<'a, RawSpinLock, T>;
