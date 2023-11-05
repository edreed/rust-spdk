use spdk_sys::{
    Errno,
    spdk_io_channel,

    spdk_put_io_channel,
};


use super::{
    Descriptor,
    Io,
};

/// A handle to a block device I/O channel.
pub struct IoChannel<'a> {
    desc: &'a Descriptor,
    channel: *mut spdk_io_channel,
}

unsafe impl Send for IoChannel<'_> {}
unsafe impl Sync for IoChannel<'_> {}

impl <'a> IoChannel<'a> {
    /// Creates a new [`IoChannel`].
    pub(crate) fn new(desc: &'a Descriptor, channel: *mut spdk_io_channel) -> Self {
        Self { desc, channel }
    }

    /// Returns the [`Descriptor`] associated with this [`IoChannel`].
    pub fn descriptor(&self) -> &Descriptor {
        self.desc
    }

    /// Returns a pointer to the underlying `spdk_io_channel` struct.
    pub fn as_ptr(&self) -> *mut spdk_io_channel {
        self.channel
    }

    /// Resets the block device zone.
    pub async fn reset_zone(&mut self, zone_id: u64) -> Result<(), Errno> {
        Io::reset_zone(&self, zone_id).await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    pub async fn write_at<B: AsRef<[u8]>>(&mut self, buf: &B, offset: u64) -> Result<(), Errno> {
        Io::write(&self, buf.as_ref(), offset).await
    }

    /// Writes zeroes to the block device at the specified offset.
    pub async fn write_zeroes_at(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::write_zeroes(&self, offset, len).await
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    pub async fn read_at<B: AsMut<[u8]>>(&mut self, buf: &mut B, offset: u64) -> Result<(), Errno> {
        Io::read(&self, buf.as_mut(), offset).await
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub async fn unmap(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::unmap(&self, offset, len).await
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub async fn flush(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::flush(&self, offset, len).await
    }

    /// Resets the block device.
    pub async fn reset(&mut self) -> Result<(), Errno> {
        Io::reset(&self).await
    }
}

impl Drop for IoChannel<'_> {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.channel) }
    }
}

