use std::{
    cell::Cell,
    env,
    ffi::{c_void, CStr, CString},
    future::Future,
    mem::{size_of, MaybeUninit},
    os::raw::{c_char, c_int},
    ptr::{addr_of_mut, null},
    sync::{atomic::AtomicBool, Arc},
};

use spdk_sys::{
    spdk_app_opts, spdk_app_opts_init, spdk_app_parse_args, spdk_app_shutdown_cb, spdk_app_start,
    spdk_app_stop, SPDK_APP_PARSE_ARGS_SUCCESS,
};
use static_init::dynamic;

use crate::{
    errors::{Errno, EINVAL},
    runtime::{reactors, Reactor},
    task::{Task, ThreadTask},
    to_result,
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
static ARGV_CSTRING: Vec<CString> = env::args().map(|a| CString::new(a).unwrap()).collect();

#[dynamic]
static APP_NAME: CString = CString::new(
    env::current_exe()
        .unwrap()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string(),
)
.unwrap();

impl Builder {
    /// Returns a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new builder with values initialized from the given
    /// [`spdk_app_opts`] struct.
    ///
    /// [`spdk_app_opts`]: ../../spdk_sys/struct.spdk_app_opts.html
    pub fn from_options(opts: spdk_app_opts) -> Self {
        let mut builder = Self(opts);

        if builder.0.name.is_null() {
            builder.with_name(&APP_NAME);
        }

        let shutdown_cb = builder.0.shutdown_cb;

        if shutdown_cb.is_none() {
            builder.with_shutdown_callback(Some(Self::shutdown));
        }

        builder
    }

    /// Returns a new builder with values initialized from the command line.
    pub fn from_cmdline() -> Result<Self, Errno> {
        let argv: Vec<*const c_char> = ARGV_CSTRING.iter().map(|a| a.as_ptr()).collect();
        let mut builder = Self::new();

        unsafe {
            let rc = spdk_app_parse_args(
                argv.len() as c_int,
                argv.as_ptr() as *mut *mut c_char,
                addr_of_mut!(builder.0),
                null(),
                null(),
                None,
                None,
            );

            if rc != SPDK_APP_PARSE_ARGS_SUCCESS {
                return Err(EINVAL);
            }
        }

        builder.with_shutdown_callback(Some(Self::shutdown));

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

    pub fn with_shutdown_callback(&mut self, callback: spdk_app_shutdown_cb) -> &mut Self {
        self.0.shutdown_cb = callback;
        self
    }

    /// Creates the configured `Runtime`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use spdk::runtime::Builder;
    /// use std::ffi::CStr;
    ///
    /// const MY_APP_NAME: &CStr = c"my_app";
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
    pub fn build(&self) -> Runtime {
        Runtime(Cell::new(self.0))
    }

    /// A callback invoked when the process receives a signal to shut down.
    unsafe extern "C" fn shutdown() {
        SHUTDOWN_STARTED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for Builder {
    fn default() -> Builder {
        let mut opts: MaybeUninit<Builder> = MaybeUninit::<Builder>::uninit();

        unsafe {
            spdk_app_opts_init(
                addr_of_mut!((*opts.as_mut_ptr()).0),
                size_of::<spdk_app_opts>(),
            );
            opts.assume_init()
        }
    }
}

static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

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
    pub fn from_cmdline() -> Result<Self, Errno> {
        Ok(Builder::from_cmdline()?
            .with_name(APP_NAME.as_c_str())
            .build())
    }

    /// Returns whether the Application Framework is shutting down.
    pub fn is_shutting_down() -> bool {
        SHUTDOWN_STARTED.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// A callback invoked to start the application's main future on the
    /// Application Framework.
    unsafe extern "C" fn handle_start<F>(ctx: *mut c_void)
    where
        F: Future<Output = ()> + 'static,
        F::Output: 'static,
    {
        let task = Arc::from_raw(ctx.cast::<ThreadTask<(), F>>());

        Task::schedule_by_ref(&task);
    }

    /// Starts the SPDK Application Framework with the given future.
    fn start<F>(&self, fut: F)
    where
        F: Future<Output = ()> + 'static,
        F::Output: 'static,
    {
        let (task, _) = ThreadTask::with_future(None, fut);
        let ctx = Arc::into_raw(task).cast_mut();

        unsafe {
            to_result!(spdk_app_start(
                self.0.as_ptr(),
                Some(Self::handle_start::<F>),
                ctx.cast()
            ))
            .expect("app started");
        }
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
    /// use spdk::runtime::Runtime;
    /// use std::ffi::CStr;
    ///
    /// const MY_APP_NAME: &CStr = c"my_app";
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
        F::Output: 'static,
    {
        struct StopGuard;

        impl Drop for StopGuard {
            fn drop(&mut self) {
                unsafe {
                    spdk_app_stop(0);
                }
            }
        }

        let wrapped_fut = async move {
            let _sg = StopGuard {};

            // Ensure each reactor has an spdk_thread associated with it and
            // collect their exit signals.
            let exits: Vec<_> = reactors().filter_map(Reactor::init).collect();

            fut.await;

            // Send exit signals to each reactor.
            exits.into_iter().for_each(|e| e.send(()).unwrap());
        };

        self.start(wrapped_fut);
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
