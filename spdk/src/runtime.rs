//! Support for Storage Performance Development Kit [Event Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
use std::{
    cell::Cell,
    env,
    ffi::{
        CStr,
        CString,
        c_void,
    },
    future::Future,
    mem::{
        MaybeUninit,
        size_of,
    },
    os::raw::{
        c_char,
        c_int,
    },
    ptr::{addr_of_mut, null}, rc::Rc,
};

use spdk_sys::{
    SPDK_APP_PARSE_ARGS_SUCCESS,

    spdk_app_opts,
    spdk_app_parse_args_rvals,

    spdk_app_opts_init,
    spdk_app_parse_args,
    spdk_app_start,
    spdk_app_stop,
};
use static_init::dynamic;

use crate::task::{
    LocalTask,
    RcTask,
};

/// Builds a runtime using the Application Framework component of the
/// [SPDK Event Framework][SPEF].
/// 
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chaining function calls. The `Runtime` object is constructed by
/// calling the [`build`] method.
///
/// [SPEF]: https://spdk.io/doc/event.html
/// [`build`]: method@Self::build
pub struct Builder(spdk_app_opts);

#[dynamic]
static ARGV_CSTRING: Vec<CString> = env::args()
    .map(|a| CString::new(a).unwrap())
    .collect();

#[dynamic]
static APP_NAME: CString = CString::new(
    env::current_exe()
        .unwrap()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string()
    ).unwrap();

impl Builder {
    /// Returns a new builder with default values.
    pub fn new() -> Self {
        default()
    }

    /// Returns a new builder with values initialized from the command line.
    pub fn from_cmdline() -> Result<Self, spdk_app_parse_args_rvals> {
        let argv: Vec<*const c_char> = ARGV_CSTRING.iter()
            .map(|a| a.as_ptr())
            .collect();
        let mut builder = default();

        unsafe {
            let rc = spdk_app_parse_args(
                argv.len() as c_int,
                argv.as_ptr() as *mut *mut c_char,
                addr_of_mut!(builder.0),
                null(),
                null(),
                None,
                None);

            if rc != SPDK_APP_PARSE_ARGS_SUCCESS {
                return Err(rc.into())
            }
        }

        Ok(builder)
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
    let mut opts: MaybeUninit<Builder> = MaybeUninit::<Builder>::uninit();

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

impl Runtime {
    /// Returns a new default runtime.
    pub fn new() -> Self {
        Builder::new().with_name(APP_NAME.as_c_str()).build()
    }

    /// Returns a new runtime initialized from the command line.
    pub fn from_cmdline()  -> Result<Self, spdk_app_parse_args_rvals> {
        Ok(Builder::from_cmdline()?.with_name(APP_NAME.as_c_str()).build())
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
    pub fn block_on<F>(&self, fut: F)
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
        
        unsafe extern "C" fn start(ctx: *mut c_void) {
            let task = Rc::from_raw(ctx.cast::<LocalTask<()>>());

            RcTask::run(&task);
        }

        let wrapped_fut = async move {
            let _sg = StopGuard{};

            fut.await
        };

        let (task, _) = LocalTask::with_future(wrapped_fut);
        let ctx = Rc::into_raw(task).cast_mut();

        unsafe {
            if spdk_app_start(self.0.as_ptr(), Some(start), ctx.cast()) != 0 {
                panic!("app failed to start");
            }
        }
    }
}