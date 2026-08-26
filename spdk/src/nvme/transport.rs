use std::{
    cmp::Ordering,
    ffi::{CStr, CString},
    fmt::{Debug, Display},
    io::{Cursor, Write},
    mem::{self, MaybeUninit},
    slice::{self},
    str::FromStr,
};

#[cfg(feature = "nvme-vfio-user")]
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use spdk_sys::{
    spdk_nvme_transport_id, spdk_nvme_transport_id_compare, spdk_nvme_transport_id_parse,
    spdk_nvme_transport_type::{self, *},
    spdk_nvme_trid_populate_transport,
    spdk_nvmf_adrfam::{SPDK_NVMF_ADRFAM_IPV4, SPDK_NVMF_ADRFAM_IPV6},
    spdk_pci_addr, spdk_pci_addr_fmt,
};
use ternary_rs::if_else;

use crate::{errors::Errno, net::SocketAddr, nvmf::TransportType, to_result};

/// The address of a device on the PCIe bus.
#[derive(Debug)]
pub struct PciAddr(spdk_pci_addr);

impl Display for PciAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf: [u8; 32] = unsafe { mem::zeroed() };

        if unsafe { spdk_pci_addr_fmt(buf.as_mut_ptr() as *mut _, buf.len(), &self.0) == 0 } {
            return f.write_str(
                (unsafe { CStr::from_bytes_with_nul_unchecked(&buf) })
                    .to_string_lossy()
                    .as_ref(),
            );
        }
        Ok(())
    }
}

/// The address of a device on a Fibre Channel SAN.
#[derive(Debug)]
pub struct FibreChannelAddr {
    node: u64,
    port: u64,
}

impl Display for FibreChannelAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "nn-{:#x}:pn-{:#x}", self.node, self.port)
    }
}

/// The transport address of an NVMe device.
#[derive(Debug)]
pub enum TransportAddr {
    /// The address of a locally-attached PCIe device.
    PCIe(Option<PciAddr>),

    /// The address of an NVMe-oF RDMA endpoint.
    RDMA(SocketAddr),

    /// The address of the FibreChannel endpoint.
    FiberChannel(FibreChannelAddr),

    /// The address of an NVMe-oF TCP endpoint.
    TCP(SocketAddr),

    /// The path to the directory containing the [`vfio-user`] device's controller Unix domain socket.
    ///
    /// See [`SPDK and libvfio-user`] for details.
    ///
    /// [`vfio-user`]: https://www.qemu.org/docs/master/system/devices/vfio-user.html
    /// [`SPDK and libvfio-user`]: https://github.com/nutanix/libvfio-user/blob/master/docs/spdk.md
    #[cfg(feature = "nvme-vfio-user")]
    VFIOUser(PathBuf),

    /// A custom transport.
    Custom(String),
}

impl From<PciAddr> for TransportAddr {
    fn from(pci_addr: PciAddr) -> Self {
        Self::PCIe(Some(pci_addr))
    }
}

impl From<Option<PciAddr>> for TransportAddr {
    fn from(pci_addr: Option<PciAddr>) -> Self {
        Self::PCIe(pci_addr)
    }
}

impl From<FibreChannelAddr> for TransportAddr {
    fn from(fc_addr: FibreChannelAddr) -> Self {
        Self::FiberChannel(fc_addr)
    }
}

/// An NVMe transport identifier.
#[derive(Clone)]
pub struct TransportId(spdk_nvme_transport_id);

impl TransportId {
    /// Creates a new [`TransportId`] for an NVMe device at the specified PCI address.
    ///
    /// If no PCI address is provided, the transport refers to the PCI bus.
    pub fn new_pcie(pci_addr: Option<&PciAddr>) -> Self {
        let mut trid = MaybeUninit::zeroed();

        unsafe {
            spdk_nvme_trid_populate_transport(trid.as_mut_ptr(), SPDK_NVME_TRANSPORT_PCIE);
        }

        // SAFETY: We can safely consider the `trid` initialized once the `trtype` field has been
        // set by `spdk_nvme_trid_populate_transport`.
        let mut trid = unsafe { trid.assume_init() };

        if let Some(pci_addr) = pci_addr {
            unsafe {
                spdk_pci_addr_fmt(trid.traddr.as_mut_ptr(), trid.traddr.len(), &pci_addr.0);
            }
        }

        Self(trid)
    }

    /// Creates a new [`TransportId`] for an NVMe device at the RDMA endpoint.
    pub fn new_rdma(addr: &SocketAddr) -> Self {
        Self::new_sockaddr(addr, SPDK_NVME_TRANSPORT_RDMA)
    }

    /// Creates a new [`TransportId`] for an NVMe device at the TCP endpoint.
    pub fn new_tcp(addr: &SocketAddr) -> Self {
        Self::new_sockaddr(addr, SPDK_NVME_TRANSPORT_TCP)
    }

    /// Creates a new [`TransportId`] for an NVMe device at the endpoint referenced by `addr`.
    fn new_sockaddr(addr: &SocketAddr, trtype: spdk_nvme_transport_type) -> Self {
        let mut trid = MaybeUninit::zeroed();

        unsafe {
            spdk_nvme_trid_populate_transport(trid.as_mut_ptr(), trtype);
        }

        // SAFETY: We can safely consider the `trid` initialized once the `trtype` field has been
        // set by `spdk_nvme_trid_populate_transport`.
        let mut trid = unsafe { trid.assume_init() };

        trid.adrfam = if_else!(addr.is_iv4(), SPDK_NVMF_ADRFAM_IPV4, SPDK_NVMF_ADRFAM_IPV6);

        let mut traddr = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.traddr.as_mut_ptr() as *mut u8, trid.traddr.len())
        });

        traddr
            .write_all(addr.ip().to_bytes_with_nul())
            .expect("NVMe transport address written");

        let mut trsvcid = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.trsvcid.as_mut_ptr() as *mut u8, trid.trsvcid.len())
        });

        write!(trsvcid, "{}\0", addr.port()).expect("NVMe transport service written");

        Self(trid)
    }

    /// Creates a new [`TransportId`] for an NVMe device at the FibreChannel endpoint.
    pub fn new_fc(fc_addr: &FibreChannelAddr) -> Self {
        let mut trid = MaybeUninit::zeroed();

        unsafe {
            spdk_nvme_trid_populate_transport(trid.as_mut_ptr(), SPDK_NVME_TRANSPORT_PCIE);
        }

        // SAFETY: We can safely consider the `trid` initialized once the `trtype` field has been
        // set by `spdk_nvme_trid_populate_transport`.
        let mut trid = unsafe { trid.assume_init() };

        let mut traddr = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.traddr.as_mut_ptr() as *mut u8, trid.traddr.len())
        });

        write!(traddr, "{}\0", fc_addr).expect("FibreChannel transport address written");

        let mut trsvcid = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.trsvcid.as_mut_ptr() as *mut u8, trid.trsvcid.len())
        });

        write!(trsvcid, "none\0").expect("FibreChannel transport service written");

        Self(trid)
    }

    /// Creates a new [`TransportId`] for an NVMe device at the specified `vfio-user` path.
    #[cfg(feature = "nvme-vfio-user")]
    pub fn new_vfio_user<P>(addr: P) -> Self
    where
        P: AsRef<Path>,
    {
        let mut trid = MaybeUninit::zeroed();

        unsafe {
            spdk_nvme_trid_populate_transport(trid.as_mut_ptr(), SPDK_NVME_TRANSPORT_VFIOUSER);
        }

        // SAFETY: We can safely consider the `trid` initialized once the `trtype` field has been
        // set by `spdk_nvme_trid_populate_transport`.
        let mut trid = unsafe { trid.assume_init() };

        let mut traddr = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.traddr.as_mut_ptr() as *mut u8, trid.traddr.len())
        });

        traddr
            .write_all(addr.as_ref().as_os_str().as_bytes())
            .expect("VFIO user transport address written");

        Self(trid)
    }

    /// Creates a new [`TransportId`] for an NVMe device at a custom endpoint.
    pub fn new_custom(addr: &str) -> Self {
        let mut trid = MaybeUninit::zeroed();

        unsafe {
            spdk_nvme_trid_populate_transport(trid.as_mut_ptr(), SPDK_NVME_TRANSPORT_CUSTOM);
        }

        // SAFETY: We can safely consider the `trid` initialized once the `trtype` field has been
        // set by `spdk_nvme_trid_populate_transport`.
        let mut trid = unsafe { trid.assume_init() };

        let mut traddr = Cursor::new(unsafe {
            slice::from_raw_parts_mut(trid.traddr.as_mut_ptr() as *mut u8, trid.traddr.len())
        });

        write!(traddr, "{}\0", addr).expect("Custom user transport address written");

        Self(trid)
    }

    /// Creates a [`TransportId`] with the specified subsystem NQN from an existing `TransportId`.
    pub fn with_subnqn(mut self, subnqn: &CStr) -> Self {
        let mut trsubnqn = Cursor::new(unsafe {
            slice::from_raw_parts_mut(self.0.subnqn.as_mut_ptr() as *mut u8, self.0.subnqn.len())
        });

        trsubnqn
            .write_all(subnqn.to_bytes_with_nul())
            .expect("transport subnqn written");

        self
    }

    /// Creates a [`TransportId`] with the specified priority from an existing `TransportId`.
    ///
    /// A prioirty is currently only supported by the Posix-based TCP socket implementation. See the
    /// documentation on the `SO_PRIORITY` socket option for [`socket`].
    ///
    /// [`socket`]: https://www.man7.org/linux/man-pages/man7/socket.7.html
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.0.priority = priority;
        self
    }

    /// Returns a pointer to the `spdk_nvme_transport_id` structure.
    pub(crate) fn as_ptr(&self) -> *const spdk_nvme_transport_id {
        &self.0
    }

    /// Returns the NVMe transport name.
    pub fn name(&self) -> &CStr {
        let trstring = unsafe {
            slice::from_raw_parts(self.0.trstring.as_ptr() as *const u8, self.0.trstring.len())
        };

        CStr::from_bytes_until_nul(trstring).expect("valid trstring")
    }

    /// Returns the NVMe transport type.
    pub fn r#type(&self) -> TransportType {
        self.0.trtype.try_into().expect("valid transport type")
    }

    /// Returns the raw transport address string.
    fn traddr(&self) -> &CStr {
        let traddr = unsafe {
            slice::from_raw_parts(self.0.traddr.as_ptr() as *const u8, self.0.traddr.len())
        };

        CStr::from_bytes_until_nul(traddr).expect("valid traddr")
    }

    /// Returns the raw transport service string.
    fn trsvcid(&self) -> &CStr {
        let trsvcid = unsafe {
            slice::from_raw_parts(self.0.trsvcid.as_ptr() as *const u8, self.0.trsvcid.len())
        };

        CStr::from_bytes_until_nul(trsvcid).expect("valid trsvcid")
    }

    /// Returns the subsystem NQN, if present.
    pub fn subnqn(&self) -> Option<&CStr> {
        let subnqn = unsafe {
            slice::from_raw_parts(self.0.subnqn.as_ptr() as *const u8, self.0.subnqn.len())
        };

        let subnqn_str = CStr::from_bytes_until_nul(subnqn).expect("valid trsvcid");

        if !subnqn_str.is_empty() {
            return Some(subnqn_str);
        }

        None
    }

    /// Returns the transport connection priority of the NVMe-oF endpoint.
    pub fn priority(&self) -> i32 {
        self.0.priority
    }
}

impl Debug for TransportId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportId")
            .field("trstring", &self.name())
            .field("trtype", &format_args!("{:x}", self.0.trtype as u32))
            .field("adrfam", &format_args!("{:x}", self.0.adrfam as u32))
            .field("traddr", &self.traddr())
            .field("trsvcid", &self.trsvcid())
            .field("subnqn", &self.subnqn())
            .field("priority", &self.0.priority)
            .finish()
    }
}

impl From<TransportAddr> for TransportId {
    fn from(trtype: TransportAddr) -> Self {
        match trtype {
            TransportAddr::PCIe(pci_addr) => TransportId::new_pcie(pci_addr.as_ref()),
            TransportAddr::RDMA(ipaddr) => TransportId::new_rdma(&ipaddr),
            TransportAddr::FiberChannel(fc_addr) => TransportId::new_fc(&fc_addr),
            TransportAddr::TCP(ipaddr) => TransportId::new_tcp(&ipaddr),
            TransportAddr::Custom(addr) => TransportId::new_custom(&addr),

            #[cfg(feature = "nvme-vfio-user")]
            TransportAddr::VFIOUser(addr) => TransportId::new_vfio_user(&addr),
        }
    }
}

impl FromStr for TransportId {
    type Err = Errno;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        unsafe {
            let s = CString::new(s).unwrap();
            let mut transport_id = MaybeUninit::zeroed();

            to_result!(spdk_nvme_transport_id_parse(
                transport_id.as_mut_ptr(),
                s.as_ptr()
            ))?;

            Ok(TransportId(transport_id.assume_init()))
        }
    }
}

impl PartialEq for TransportId {
    fn eq(&self, other: &Self) -> bool {
        unsafe { spdk_nvme_transport_id_compare(&self.0, &other.0) == 0 }
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
