use spdk_sys::spdk_thread_get_app_thread;

use super::AsRawThread;

/// Represents the SPDK application thread.
pub struct App;

unsafe impl Send for App {}
unsafe impl Sync for App {}

impl AsRawThread for App {
    fn as_raw_thread(&self) -> *mut spdk_sys::spdk_thread {
        let ptr = unsafe { spdk_thread_get_app_thread() };

        assert!(
            !ptr.is_null(),
            "SPDK Application Framework must be initialized"
        );

        ptr
    }
}
