use std::{
    cmp::Ordering,
    ffi::{CStr, CString},
    fmt::{self, Debug, Display},
    mem::MaybeUninit,
    str::FromStr,
};

use spdk_sys::{
    SPDK_UUID_STRING_LEN, spdk_uuid, spdk_uuid_compare, spdk_uuid_copy, spdk_uuid_fmt_lower,
    spdk_uuid_generate, spdk_uuid_is_null, spdk_uuid_parse, spdk_uuid_set_null,
};

use crate::{
    errors::{EINVAL, Errno},
    to_result,
};

/// A Universally Unique Identifier (UUID) implemented by the SPDK.
pub struct Uuid(spdk_uuid);

unsafe impl Send for Uuid {}

impl Uuid {
    /// Generate a new UUID.
    ///
    /// # Examples
    ///
    /// ```
    /// let uuid = Uuid::new();
    /// assert!(!uuid.is_null());
    /// ```
    pub fn new() -> Self {
        let mut uuid = MaybeUninit::<spdk_uuid>::uninit();

        unsafe { spdk_uuid_generate(uuid.as_mut_ptr()) };

        Self(unsafe { uuid.assume_init() })
    }

    /// Set the UUID to null.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut uuid = Uuid::new();
    /// uuid.set_null();
    /// assert!(uuid.is_null());
    /// ```
    pub fn is_null(&self) -> bool {
        unsafe { spdk_uuid_is_null(&self.0) }
    }

    /// Set the UUID to null.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut uuid = Uuid::new();
    /// uuid.set_null();
    /// assert!(uuid.is_null());
    /// ```
    pub fn set_null(&mut self) {
        unsafe {
            spdk_uuid_set_null(&mut self.0);
        }
    }

    /// Get a raw pointer to the underlying `spdk_uuid` struct.
    #[cfg(feature = "bdev")]
    pub(crate) fn as_ptr(&self) -> *const spdk_uuid {
        &self.0 as *const spdk_uuid
    }

    /// Create a `Uuid` from a raw pointer to an `spdk_uuid`.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and point to a valid `spdk_uuid` instance.
    #[cfg(feature = "bdev")]
    pub(crate) unsafe fn from_ptr_unchecked(ptr: *const spdk_uuid) -> Self {
        Self(unsafe { *ptr })
    }
}

impl Default for Uuid {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0i8; SPDK_UUID_STRING_LEN as usize];

        let cstr = unsafe {
            to_result!(spdk_uuid_fmt_lower(buf.as_mut_ptr(), buf.len(), &self.0))
                .expect("UUID rendered");

            // SAFETY: `spdk_uuid_fmt_lower` writes a null-terminated string to `buf`.
            CStr::from_ptr(buf.as_ptr())
        };

        write!(f, "{}", cstr.to_string_lossy())
    }
}

impl Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Uuid")
            .field(&format_args!("{}", self))
            .finish()
    }
}

impl From<spdk_uuid> for Uuid {
    fn from(uuid: spdk_uuid) -> Self {
        Self(uuid)
    }
}

impl FromStr for Uuid {
    type Err = Errno;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = CString::new(s).map_err(|_| EINVAL)?;
        let mut uuid = MaybeUninit::<spdk_uuid>::uninit();

        unsafe { to_result!(spdk_uuid_parse(uuid.as_mut_ptr(), s.as_ptr()))? };

        Ok(Uuid(unsafe { uuid.assume_init() }))
    }
}

impl Clone for Uuid {
    fn clone(&self) -> Self {
        let mut clone = MaybeUninit::<spdk_uuid>::uninit();

        unsafe { spdk_uuid_copy(clone.as_mut_ptr(), &self.0) };

        Self(unsafe { clone.assume_init() })
    }
}

impl Ord for Uuid {
    fn cmp(&self, other: &Self) -> Ordering {
        match unsafe { spdk_uuid_compare(&self.0, &other.0) } {
            0 => Ordering::Equal,
            x if x < 0 => Ordering::Less,
            _ => Ordering::Greater,
        }
    }
}

impl PartialOrd for Uuid {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Uuid {
    fn eq(&self, other: &Self) -> bool {
        unsafe { spdk_uuid_compare(&self.0, &other.0) == 0 }
    }
}

impl Eq for Uuid {}
