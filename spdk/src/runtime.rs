use std::{
    ffi::{CStr, c_void},
    mem::{size_of, MaybeUninit},
    ptr::{addr_of_mut},
    future::Future,
    cell::Cell
};

use async_task::Runnable;
use byte_strings::c_str;
use spdk_sys::{
    spdk_app_opts,
    spdk_app_opts_init,
    spdk_app_stop,
    spdk_app_start
    };

use crate::thread::Thread;

/// Builds a runtime using the Application Framework component of the
/// [SPDK Event Framework][SPEF].
/// 
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chanining function calls. The `Runtime` object is constructed by
/// calling the [`build`] method.
///
/// [SPEF]: https://spdk.io/doc/event.html
/// [`build`]: method@Self::build
pub struct Builder(spdk_app_opts);

impl Builder {
    /// Returns a new builder with default values.
    pub fn new() -> Self {
        default()
    }

    /// Sets the runtime name.
    pub fn with_name(&mut self, name: &CStr) -> &mut Self {
        self.0.name = name.as_ptr();
        self
    }

    /// Sets the configuration file used to initialize SPDK subsystems.
    /// 
    /// The subsystem configuration file is a JSON formatted file. The
    /// format is documented in [`$SPDK/lib/init/json_config.c`][SCFG].
    /// 
    /// [SCFG]: https://github.com/spdk/spdk/blob/master/lib/init/json_config.c
    pub fn with_config_file(&mut self, path: &CStr) -> &mut Self {
        self.0.json_config_file = path.as_ptr();
        self
    }

    /// Sets whether to ignore errors parsing the subsystem configuration
    /// file.
    pub fn ignore_config_errors(&mut self, ignore: bool) -> &mut Self {
        self.0.json_config_ignore_errors = ignore;
        self
    }

    /// Sets whether to enable coredumps.
    pub fn enable_coredump(&mut self, enable: bool) -> &mut Self {
        self.0.enable_coredump = enable;
        self
    }

    /// Creates the configured `Runtime`.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use byte_strings::c_str;
    /// use spdk::runtime::Builder;
    /// use std::ffi::CStr;
    /// 
    /// const MY_APP_NAME: &CStr = c_str!("my_app");
    /// 
    /// fn main() {
    ///     let rt = Builder::new()
    ///         .with_name(MY_APP_NAME)
    ///         .build();
    /// 
    ///     rt.block_on(async {
    ///         println!("Hello, World!");
    ///     });
    /// }
    /// ```
    pub fn build(&mut self) -> Runtime {
        Runtime(Cell::new(self.0))
    }

}

/// Returns a new builder with default values.
pub fn default() -> Builder {
    let mut opts = MaybeUninit::<Builder>::uninit();

    unsafe {
        spdk_app_opts_init(addr_of_mut!((*opts.as_mut_ptr()).0), size_of::<spdk_app_opts>());
        opts.assume_init()
    }
}

/// A runtime implemented using the Application Framework component of the
/// [SPDK Event Framework][SPEF].
/// 
/// [SPEF]: https://spdk.io/doc/event.html
pub struct Runtime(Cell<spdk_app_opts>);

const NAME: &CStr = c_str!("spdk_app");

impl Runtime {
    /// Returns a new default runtime.
    pub fn new() -> Self {
        Builder::new().with_name(NAME).build()
    }

    /// Runs a future to completion on the SPDK Application Framework.
    /// 
    /// This function initializes the SPDK Application Framework and runs the
    /// given future to completion on the current thread. Any tasks which the
    /// future spawns internally will be executed on this runtime.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use byte_strings::c_str;
    /// use spdk::runtime::Runtime;
    /// use std::ffi::CStr;
    /// 
    /// const MY_APP_NAME: &CStr = c_str!("my_app");
    /// 
    /// fn main() {
    ///     let rt = Runtime::new();
    /// 
    ///     rt.block_on(async {
    ///         println!("Hello, World!");
    ///     });
    /// }
    /// ```
    pub fn block_on<F>(&self, f: F)
    where
        F: Future<Output = ()> + 'static,
        F::Output: 'static
    {
        struct StopGuard;

        impl Drop for StopGuard {
            fn drop(&mut self) {
                unsafe { spdk_app_stop(0); }
            }
        }
        
        let future = async move {
            let _sg = StopGuard{};

            f.await
        };

        let schedule = move |runnable: Runnable| Thread::current().send_msg(|| _ = runnable.run()).unwrap();
        let (runnable, _task) = async_task::spawn_local(future, schedule);

        struct StartMsg {
            start: Box<dyn FnOnce()>
        }

        unsafe extern "C" fn start(ctx: *mut c_void) {
            let start_msg = Box::from_raw(ctx as *mut StartMsg);

            (start_msg.start)();
        }

        let start_msg = StartMsg{ start: Box::new(move || _ = runnable.run()) };
        let ctx = Box::into_raw(Box::new(start_msg)).cast();

        unsafe {
            if spdk_app_start(self.0.as_ptr(), Some(start), ctx) != 0 {
                panic!("app failed to start");
            }
        }
    }
}