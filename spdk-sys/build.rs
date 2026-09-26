use std::{
    env,
    fs::canonicalize,
    path::{Path, PathBuf},
};

use bindgen::callbacks::ParseCallbacks;
use fs_extra::dir;
use itertools::Itertools;
use lazy_regex::regex_replace_all;
use pkg_config::Library;
use ternary_rs::if_else;

use crate::{
    AutotoolsOption::{Disable, With},
    BindgenOption::{
        AllowFunction, AllowType, AllowVar, BlockType, ConstifiedEnumModule, OpaqueType,
        RustifiedEnum, RustifiedNonExhaustiveEnum,
    },
};

/// Implements the [`ParseCallbacks`] trait to workaround issues in the [`doxygen_rs`] crate's handling
/// of SPDK doc comments.
#[derive(Debug)]
struct DoxygenCallbacks;

impl DoxygenCallbacks {
    fn new() -> Box<Self> {
        Box::new(Self)
    }
}

impl ParseCallbacks for DoxygenCallbacks {
    fn process_comment(&self, comment: &str) -> Option<String> {
        // The following workaround is to prevent the doxygen_rs crate from panicking when it
        // encounters a Doxygen comment on the end of a line.
        //
        // TODO: Fix the doxygen_rs crate to not panic on when Doxygen comments are on the end of
        // the line.
        let comment = regex_replace_all!(r"([\\@]\w+\b*)\n", comment, "$1");

        let transformed = doxygen_rs::transform(&comment);

        // The doxygen-rs crate doesn't handle punctuation following a link in Doxygen comments
        // correctly, including it in the link. The following workaround removes the punctuation
        // from the link.
        let transformed =
            regex_replace_all!(r"\[`((?:\w+)(?:\(\))?)(\W+)\`]", &transformed, "[`$1`]$2");

        // RustDoc assumes anything between `[` & `]` is a link and tries to resolve it. The
        // following workaround escapes these brackets when the contained symbol does not appear to
        // be an SPDK symbol.
        let transformed = regex_replace_all!(r"\[(\w+)\]", &transformed, r"\[$1\]");

        Some(transformed.into())
    }
}

/// An enumeration of [`autotools`] configuration options.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
enum AutotoolsOption<'a> {
    /// Pass `--enable-<opt>[=<optarg>]` to the `configure` script.
    Enable(&'a str, Option<&'a str>),

    /// Pass `--disable-<opt>[=<optarg>]` to the `configure` script.
    Disable(&'a str, Option<&'a str>),

    /// Pass `--with-<opt>[=<optarg>]` to the `configure` script.
    With(&'a str, Option<&'a str>),

    /// Pass `--without-<opt>[=<optarg>]` to the `configure` script.
    Without(&'a str, Option<&'a str>),
}

impl AutotoolsOption<'_> {
    /// Apply this option to the [`autotools::Config`].
    fn apply(&self, config: &mut autotools::Config) {
        match self {
            Self::Enable(opt, optarg) => _ = config.enable(opt, optarg.as_ref()),
            Self::Disable(opt, optarg) => _ = config.disable(opt, optarg.as_ref()),
            Self::With(opt, optarg) => _ = config.with(opt, optarg.as_ref()),
            Self::Without(opt, optarg) => _ = config.with(opt, optarg.as_ref()),
        }
    }
}

/// An enumeration of [`bindgen::Builder`] options.
#[derive(Copy, Clone, Debug)]
enum BindgenOption<'a> {
    /// Generate bindings for the specified function(s).
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::allowlist_function`] for details.
    AllowFunction(&'a str),

    /// Generate bindings for the specified type(s).
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::allowlist_type`] for details.
    AllowType(&'a str),

    /// Generate bindings for the specified variable(s).
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::allowlist_var`] for details.
    AllowVar(&'a str),

    /// Do not generate any bindings for the specified function(s).
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::blocklist_type`] for details.
    BlockType(&'a str),

    /// Treat the specified type(s) as opaque in the generated bindings.
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::opaque_type`] for details.
    OpaqueType(&'a str),

    /// Mark the specified enum(s) as a module with a set of integer constants.
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::constified_enum_module`] for details.
    ConstifiedEnumModule(&'a str),

    /// Mark the specified enum(s) as a Rust enum.
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::constified_enum_module`] for details.
    RustifiedEnum(&'a str),

    /// Mark the specified enum(s) as a non-exhaustive Rust enum.
    ///
    /// Regular expressions are supported. See [`bindgen::Builder::constified_enum_module`] for details.
    RustifiedNonExhaustiveEnum(&'a str),
}

impl BindgenOption<'_> {
    fn apply(&self, bindgen: bindgen::Builder) -> bindgen::Builder {
        match self {
            Self::AllowFunction(fun) => bindgen.allowlist_function(fun),
            Self::AllowType(typ) => bindgen.allowlist_type(typ),
            Self::AllowVar(var) => bindgen.allowlist_var(var),
            Self::BlockType(typ) => bindgen.blocklist_type(typ),
            Self::OpaqueType(typ) => bindgen.opaque_type(typ),
            Self::ConstifiedEnumModule(typ) => bindgen.constified_enum_module(typ),
            Self::RustifiedEnum(typ) => bindgen.rustified_enum(typ),
            Self::RustifiedNonExhaustiveEnum(typ) => bindgen.rustified_non_exhaustive_enum(typ),
        }
    }
}

/// Build configuration options for an `spdk-sys` crate feature.
#[derive(Copy, Clone, Debug)]
struct Feature<'a> {
    /// The build environment variable name of the feature.
    ///
    /// See [The Cargo Book / Features / Build scripts] for details on the name format.
    ///
    /// [The Cargo Book / Features / Build scripts]: https://doc.rust-lang.org/cargo/reference/features.html#build-scripts
    name: &'a str,

    /// The [`autotools`] configuration options.
    autotools_opts: &'a [AutotoolsOption<'a>],

    /// The [`pkg_config`] libraries to include in the build output.
    pkgconfigs: &'a [&'a str],

    /// The [Cargo link-lib instructions] for additional libraries, if any, to link into the output
    /// binary.
    ///
    /// [Cargo link-lib instructions]:
    ///     https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib
    additional_link_libs: &'a [&'a str],

    /// The additional paths to search for libraries.
    additional_link_paths: &'a [&'a str],

    /// The additional paths to search for include files.
    additional_include_paths: &'a [&'a str],

    /// The [`bindgen`] configuration options.
    bindgen_opts: &'a [BindgenOption<'a>],
}

impl<'a> Feature<'a> {
    /// Creates a new [`Feature`] instance.
    const fn new(
        name: &'a str,
        autotools_opts: &'a [AutotoolsOption<'a>],
        pkgconfigs: &'a [&'a str],
        additional_link_libs: &'a [&'a str],
        additional_link_paths: &'a [&'a str],
        additional_include_paths: &'a [&'a str],
        bindgen_opts: &'a [BindgenOption<'a>],
    ) -> Self {
        Self {
            name,
            autotools_opts,
            pkgconfigs,
            additional_link_libs,
            additional_link_paths,
            additional_include_paths,
            bindgen_opts,
        }
    }

    /// Returns the name of the feature.
    ///
    /// The name has the form `CARGO_FEATURE_<XXX>` where `<XXX>` is the upper snake case name of
    /// the feature.
    fn name(&self) -> &str {
        self.name
    }

    /// Returns whether the feature is enabled.
    fn is_enabled(&self) -> bool {
        env::var_os(self.name).is_some()
    }

    /// Returns the [`autotools`] configuration options.
    fn autotools_opts(&self) -> &[AutotoolsOption<'_>] {
        self.autotools_opts
    }

    /// Returns the [`pkg_config`] libraries to include in the build output.
    fn pkgconfigs(&self) -> &[&str] {
        self.pkgconfigs
    }

    /// Returns [Cargo link-lib instructions] for additional libraries, if any, to link into the output
    /// binary.
    ///
    /// [Cargo link-lib instructions]:
    ///     https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib
    fn additional_link_libs(&self) -> &[&str] {
        self.additional_link_libs
    }

    /// Returns additional paths to search for libraries.
    fn additional_link_paths(&self) -> &[&str] {
        self.additional_link_paths
    }

    /// Returns additional paths to search for include files.
    fn additional_include_paths(&self) -> &[&str] {
        self.additional_include_paths
    }

    /// Returns the [`bindgen`] configuration options.
    fn bindgen_opts(&self) -> &[BindgenOption<'_>] {
        self.bindgen_opts
    }
}

/// An array of all features supported by the `spdk-sys` crate.
const ALL_FEATURES: [Feature; 12] = [
    Feature::new(
        "CARGO_FEATURE_BASE",
        [
            Disable("apps", None),
            Disable("examples", None),
            Disable("tests", None),
            Disable("unit-tests", None),
        ]
        .as_slice(),
        ["spdk_env_dpdk", "spdk_event", "spdk_syslibs"].as_slice(),
        [
            "cargo:rustc-link-lib=static:+whole-archive,-bundle=isal",
            "cargo:rustc-link-lib=static:+whole-archive,-bundle=isal_crypto",
        ]
        .as_slice(),
        ["spdk/isa-l/.libs", "spdk/isa-l-crypto/.libs"].as_slice(),
        ["spdk/include", "spdk/module"].as_slice(),
        [
            AllowFunction("spdk_.*"),
            AllowType("spdk_.*"),
            AllowVar("SPDK_.*"),
        ]
        .as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_BDEV",
        [].as_slice(),
        ["spdk_bdev", "spdk_event_bdev"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [
            OpaqueType("spdk_nvme_(ctrlr|health|sgl|tcp)_.*"),
            OpaqueType("spdk_bdev_ext_io_opts"),
            RustifiedEnum("spdk_dif_.*"),
            RustifiedEnum("spdk_bdev_io_(status|type)"),
        ]
        .as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_BDEV_AIO",
        [].as_slice(),
        ["spdk_bdev_aio"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [AllowFunction(r"\w+_aio_\w+")].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_BDEV_MALLOC",
        [].as_slice(),
        ["spdk_bdev_malloc"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [
            AllowFunction(".*_malloc_disk"),
            AllowType("malloc_bdev_opts"),
        ]
        .as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_BDEV_MODULE",
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_BDEV_URING",
        [With("uring", None)].as_slice(),
        ["spdk_bdev_uring"].as_slice(),
        ["cargo:rustc-link-lib=static=uring"].as_slice(),
        ["/usr/lib64"].as_slice(),
        [].as_slice(),
        [AllowFunction(r"\w+_uring_\w+"), AllowType(r"\w+_uring_\w+")].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_JSON",
        [].as_slice(),
        ["spdk_json"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_NET",
        [].as_slice(),
        ["spdk_sock", "spdk_event_sock"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_NVME",
        [].as_slice(),
        ["spdk_nvme"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [
            BlockType("spdk_nvme_cdata_(fuses|oncs)"),
            ConstifiedEnumModule(r"spdk_nvme_(\w+)?status_code(_type)?"),
            RustifiedNonExhaustiveEnum("spdk_nvme_transport_type"),
        ]
        .as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_NVME_VFIO_USER",
        [AutotoolsOption::With("vfio-user", None)].as_slice(),
        ["spdk_vfio_user"].as_slice(),
        ["cargo:rustc-link-lib=static=vfio-user"].as_slice(),
        ["spdk/build/libvfio-user/usr/local/lib"].as_slice(),
        ["spdk/build/libvfio-user/usr/local/include"].as_slice(),
        [].as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_NVMF",
        [].as_slice(),
        ["spdk_nvmf", "spdk_event_nvmf"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [
            AllowVar("g_spdk_.*"),
            OpaqueType("spdk_nvmf_(fabric_.*|discovery_log_page_entry)"),
            RustifiedNonExhaustiveEnum("spdk_nvmf_.*"),
        ]
        .as_slice(),
    ),
    Feature::new(
        "CARGO_FEATURE_SCSI",
        [].as_slice(),
        ["spdk_scsi"].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [].as_slice(),
        [ConstifiedEnumModule(r"spdk_scsi_(asc|ascq|sense|status)")].as_slice(),
    ),
];

/// The features currently enabled for this build.
#[derive(Debug)]
struct Features<'a> {
    features: Vec<Feature<'a>>,
}

impl<'a> Features<'a> {
    /// Creates a new [`Features`] instance with information on enabled features for this build.
    fn new(features: &'a [Feature]) -> Self {
        let features = features
            .iter()
            .filter(|f| f.is_enabled())
            .copied()
            .collect();

        Self { features }
    }

    /// Applies the [`autotools`] configuration for enabled features.
    fn apply_autotools_options(&self, config: &mut autotools::Config) {
        self.features
            .iter()
            .flat_map(|f| f.autotools_opts().iter())
            .for_each(|f| f.apply(config));
    }

    /// Returns the [`pkg_config`] libraries required by the enabled features.
    fn libraries(&self, pkgconfig: &pkg_config::Config) -> Vec<Library> {
        self.features
            .iter()
            .flat_map(|f| f.pkgconfigs().iter().copied())
            .filter_map(|cfg| {
                pkgconfig
                    .probe(cfg)
                    .inspect_err(|err| {
                        println!(
                            "cargo::error=\"Failed to find the {} package config: {}\"",
                            cfg,
                            err.to_string().trim_start_matches(char::is_whitespace)
                        )
                    })
                    .ok()
            })
            .collect()
    }

    /// Returns [Cargo link-lib instructions] for additional libraries, if any, to link into the output
    /// binary.
    ///
    /// [Cargo link-lib instructions]:
    ///     https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib
    fn additional_link_libs(&self) -> Vec<String> {
        self.features
            .iter()
            .flat_map(|f| f.additional_link_libs().iter().copied().map(Into::into))
            .collect()
    }

    /// Returns additional paths to search for libraries.
    fn additional_link_paths<P>(&self, out_dir: P, profile: BuildProfile) -> Vec<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.features
            .iter()
            .flat_map(|f| {
                f.additional_link_paths().iter().copied().map(|p| {
                    out_dir
                        .as_ref()
                        .join(p.replace("${profile}", profile.as_str()))
                })
            })
            .collect()
    }

    /// Returns additional paths to search for include files.
    fn additional_include_paths<P>(&self, out_dir: P, profile: BuildProfile) -> Vec<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.features
            .iter()
            .flat_map(|f| {
                f.additional_include_paths().iter().copied().map(|p| {
                    out_dir
                        .as_ref()
                        .join(p.replace("${profile}", profile.as_str()))
                })
            })
            .collect()
    }

    /// Returns the macro definitions required by the enabled features.
    fn defines(&self) -> Vec<String> {
        self.features
            .iter()
            .map(|f| format!("{}=1", f.name()))
            .collect()
    }

    /// Applies the [`bindgen`] configuration options.
    fn apply_bindgen_options(&self, bindgen: bindgen::Builder) -> bindgen::Builder {
        self.features
            .iter()
            .flat_map(|f| f.bindgen_opts())
            .fold(bindgen, |bindgen, f| f.apply(bindgen))
    }
}

/// The current build profile (i.e. "debug" or "release").
#[derive(Copy, Clone, Debug)]
enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

/// Orchestrates the Storage Performance Development Kit (SPDK) build.
struct Builder<'a> {
    /// The features currently enabled for this build.
    features: Features<'a>,

    /// The directory containing SPDK sources.
    spdk_src_dir: PathBuf,

    /// The build output directory.
    out_dir: PathBuf,

    /// The build profile, i.e. "debug" or "release".
    profile: BuildProfile,
}

impl Builder<'_> {
    /// Creates a new [`Builder`] instance.
    fn new() -> Self {
        Self {
            features: Features::new(ALL_FEATURES.as_slice()),
            spdk_src_dir: canonicalize("spdk").expect("spdk submodule to be initialized"),
            out_dir: PathBuf::from(env::var_os("OUT_DIR").expect("$OUT_DIR set by build")),
            profile: if_else!(
                env::var("DEBUG").unwrap_or("false".into()).parse().unwrap(),
                BuildProfile::Debug,
                BuildProfile::Release
            ),
        }
    }

    /// Builds the SPDK.
    fn build_spdk(&self) {
        let spdk_dir = self.out_dir.join("spdk");

        if !spdk_dir.exists() {
            let copy_options = dir::CopyOptions::new().overwrite(true);

            dir::copy(&self.spdk_src_dir, &self.out_dir, &copy_options)
                .expect("$OUT_DIR is writeable");
        }

        let spdk_pkgconfig_dir = spdk_dir.join("build/lib/pkgconfig");

        if !spdk_pkgconfig_dir.exists() {
            let mut config = autotools::Config::new(spdk_dir);

            config
                .forbid("--disable-shared")
                .forbid("--enable-static")
                .config_option("prefix", Some(""))
                .insource(true);

            self.features.apply_autotools_options(&mut config);

            if env::var("DEBUG").unwrap_or("false".into()).parse().unwrap() {
                config.enable("debug", None);
            } else {
                config.disable("debug", None);
            }

            let _dst = config.make_target("all").build();
        }
    }

    /// Emits the Cargo build instructions to find and link SPDK and other library dependencies into
    /// the output binary, and returns the include search paths and features defines need to create
    /// the FFI bindings.
    fn emit_dependencies(&self) -> (Vec<PathBuf>, Vec<String>) {
        let spdk_pkgconfig_dir = self.out_dir.join("spdk/build/lib/pkgconfig");
        let old_pkg_config_path = env::var("PKG_CONFIG_PATH").unwrap_or("".into());

        unsafe {
            env::set_var(
                "PKG_CONFIG_PATH",
                format!(
                    "{}:{}",
                    old_pkg_config_path,
                    spdk_pkgconfig_dir.to_str().unwrap()
                ),
            )
        };

        let mut pkg_config = pkg_config::Config::new();

        pkg_config
            .cargo_metadata(false)
            .env_metadata(false)
            .statik(true);

        let libraries = self.features.libraries(&pkg_config);

        let link_paths: Vec<PathBuf> = libraries
            .iter()
            .flat_map(|l| l.link_paths.iter())
            .unique()
            .cloned()
            .chain(
                self.features
                    .additional_link_paths(&self.out_dir, self.profile),
            )
            .collect();

        link_paths
            .iter()
            .for_each(|p| println!("cargo:rustc-link-search=native={}", p.to_str().unwrap()));

        let include_paths: Vec<PathBuf> = libraries
            .iter()
            .flat_map(|l| l.include_paths.iter())
            .unique()
            .cloned()
            .chain(
                self.features
                    .additional_include_paths(&self.out_dir, self.profile),
            )
            .collect();

        libraries
            .iter()
            .flat_map(|library| {
                library.libs.iter().map(|lib| {
                    let libname = format!("lib{}.a", lib);

                    if library.link_paths.iter().any(|p| p.join(&libname).exists()) {
                        format!("cargo:rustc-link-lib=static:+whole-archive,-bundle={}", lib)
                    } else {
                        format!("cargo:rustc-link-lib=dylib={}", lib)
                    }
                })
            })
            .chain(self.features.additional_link_libs())
            .unique()
            .for_each(|l| println!("{l}"));

        (include_paths, self.features.defines())
    }

    /// Generates the SPDK FFI bindings.
    fn generate_bindings(&self, include_paths: &[PathBuf], defines: &[String]) {
        let spdk_wrappers = self.out_dir.join("spdk/build/wrappers.c");
        let builder = bindgen::Builder::default()
            .header("wrapper.h")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .parse_callbacks(DoxygenCallbacks::new())
            .clang_args(
                include_paths
                    .iter()
                    .map(|i| format!("-I{}", i.to_string_lossy())),
            )
            .clang_args(defines.iter().map(|d| format!("-D{}", d)))
            .wrap_static_fns(true)
            .wrap_static_fns_path(&spdk_wrappers)
            .wrap_unsafe_ops(true)
            .prepend_enum_name(false)
            .generate_cstr(true)
            .layout_tests(false);

        let _ = self
            .features
            .apply_bindgen_options(builder)
            .generate()
            .expect("spdk bindings generated")
            .write_to_file(self.out_dir.join("bindings.rs").as_path());
    }

    /// Builds the macro function wrappers and extra function into the `spdk_extras` library.
    fn build_extras(&self, include_paths: &[PathBuf], defines: &[String]) {
        let spdk_wrappers = self.out_dir.join("spdk/build/wrappers.c");
        let mut extras = cc::Build::new();

        if spdk_wrappers.exists() {
            extras.file(spdk_wrappers);
        }

        extras
            .file("ext.c")
            .include(".")
            .includes(include_paths)
            .flag("-Wno-unused-parameter");

        defines.iter().for_each(|d| _ = extras.define(d, None));

        extras.compile("spdk_extras");
    }
}

/// The build script to build the SPDK, generate FFI bindings and emit build instructions for
/// link-time dependencies.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=ext.c");

    let builder = Builder::new();

    builder.build_spdk();

    let (include_paths, defines) = builder.emit_dependencies();

    builder.generate_bindings(&include_paths, &defines);

    builder.build_extras(&include_paths, &defines);
}
