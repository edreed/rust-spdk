use std::{str::FromStr, mem::MaybeUninit, ffi::CString, cmp::Ordering};

use spdk_sys::{
    Errno,
    spdk_nvme_transport_id,

    to_result,

    spdk_nvme_transport_id_compare,
    spdk_nvme_transport_id_parse,
};

#[derive(Clone)]
pub struct TransportId(spdk_nvme_transport_id);

impl TransportId {
    pub(crate) fn as_ptr(&self) -> *const spdk_nvme_transport_id {
        &self.0
    }
}

impl FromStr for TransportId {
    type Err = Errno;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        unsafe {
            let s = CString::new(s).unwrap();
            let mut transport_id = MaybeUninit::uninit();

            to_result!(spdk_nvme_transport_id_parse(
                transport_id.as_mut_ptr(),
                s.as_ptr()))?;

            Ok(TransportId(transport_id.assume_init()))
        }
    }
}

impl PartialEq for TransportId {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            spdk_nvme_transport_id_compare(&self.0, &other.0) == 0
        }
    }
}

impl Eq for TransportId {}

impl Ord for TransportId {
    fn cmp(&self, other: &Self) -> Ordering {
        unsafe {
            match spdk_nvme_transport_id_compare(&self.0, &other.0) {
                0 => Ordering::Equal,
                x if x < 0 => Ordering::Less,
                _ => Ordering::Greater,
            }
        }
    }
}

impl PartialOrd for TransportId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
