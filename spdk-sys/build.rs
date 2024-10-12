use std::{
    env,
    fs::canonicalize,
    iter::once,
    path::PathBuf,
};

use bindgen::callbacks::ParseCallbacks;
use fs_extra::dir;
use itertools::Itertools;
use regex::Regex;

#[derive(Debug)]
struct DoxygenCallbacks {
    eol_doxygen: Regex,
}

impl DoxygenCallbacks {
    fn new() -> Self {
        Self {
            eol_doxygen: Regex::new(r"([\\@]\w+\b*)\n").expect("eol_doxygen regex to compile"),
        }
    }
}

impl ParseCallbacks for DoxygenCallbacks {
    fn process_comment(&self, comment: &str) -> Option<String> {
        // The following workaround is to prevent the doxygen_rs crate from
        // panicking when it encounters a Doxygen comment on the end of a line.
        //
        // TODO: Fix the doxygen_rs crate to not panic on when Doxygen comments
        // are on the end of the line.
        let comment = self.eol_doxygen.replace_all(comment, "$1");

        Some(doxygen_rs::transform(&comment))
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");

    let spdk_src_dir = canonicalize("spdk").expect("spdk submodule to be initialized");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("$OUT_DIR set by build"));
    let spdk_dir = out_dir.join("spdk");
    let spdk_lib_dir = spdk_dir.join("build/lib");
    let spdk_pkgconfig_dir = spdk_lib_dir.join("pkgconfig");
    let spdk_include_dir = spdk_src_dir.join("include");
    let spdk_module_dir = spdk_src_dir.join("module");
    let isal_lib_dir = spdk_dir.join("isa-l/.libs");
    let isal_crypto_lib_dir = spdk_dir.join("isa-l-crypto/.libs");
    let spdk_wrappers = spdk_dir.join("build/wrappers.c");

    if !spdk_dir.exists() {
        let copy_options = dir::CopyOptions::new()
            .overwrite(true);

        dir::copy(spdk_src_dir, out_dir.clone(), &copy_options).expect("$OUT_DIR is writeable");
    }

    if !spdk_pkgconfig_dir.exists() {
        let mut config = autotools::Config::new(spdk_dir);
        config
            .forbid("--disable-shared")
            .forbid("--enable-static")
            .disable("apps", None)
            .disable("examples", None)
            .disable("tests", None)
            .disable("unit-tests", None)
            .config_option("prefix", Some(""))
            .insource(true)
            .make_target("all");

        if env::var("DEBUG").unwrap_or("false".into()).parse().unwrap() {
            config.enable("debug", None);
        } else {
            config.disable("debug", None);
        }

        let _dst = config.build();
    }

    let old_pkg_config_path = env::var("PKG_CONFIG_PATH").unwrap_or("".into());

    env::set_var("PKG_CONFIG_PATH", format!("{}:{}", old_pkg_config_path, spdk_pkgconfig_dir.to_str().unwrap()));

    let mut pkg_config = pkg_config::Config::new();
    pkg_config
        .cargo_metadata(false)
        .env_metadata(false)
        .statik(true);

    let mut pkg_configs = vec![
        pkg_config.probe("spdk_env_dpdk").expect("spdk_env_dpdk package config exists"),
        pkg_config.probe("spdk_event").expect("spdk_event package config exists"),
        pkg_config.probe("spdk_syslibs").expect("spdk_syslibs package config exists"),
    ];

    let mut defines = Vec::<&str>::new();
    let include_bdev = env::var_os("CARGO_FEATURE_BDEV").is_some();

    if include_bdev {
        pkg_configs.push(pkg_config.probe("spdk_bdev").expect("spdk_bdev package config exists"));
        pkg_configs.push(pkg_config.probe("spdk_event_bdev").expect("spdk_event_bdev package config exists"));
        defines.push("CARGO_FEATURE_BDEV=1");
    }

    let include_bdev_module = env::var_os("CARGO_FEATURE_BDEV_MODULE").is_some();

    if include_bdev_module {
        defines.push("CARGO_FEATURE_BDEV_MODULE=1");
    }

    let include_bdev_malloc = env::var_os("CARGO_FEATURE_BDEV_MALLOC").is_some();

    if include_bdev_malloc {
        pkg_configs.push(pkg_config.probe("spdk_bdev_malloc").expect("spdk_bdev_malloc package config exists"));
        defines.push("CARGO_FEATURE_BDEV_MALLOC=1");
    }

    let include_target_nvmf = env::var_os("CARGO_FEATURE_NVMF").is_some();

    if include_target_nvmf {
        pkg_configs.push(pkg_config.probe("spdk_nvmf").expect("spdk_nvmf package config exists"));
        pkg_configs.push(pkg_config.probe("spdk_event_nvmf").expect("spdk_event_nvmf package config exists"));
        defines.push("CARGO_FEATURE_NVMF=1");
    }

    let link_paths: Vec<PathBuf> = pkg_configs
        .iter()
        .flat_map(|l| l.link_paths.iter())
        .chain(once(&isal_lib_dir))
        .chain(once(&isal_crypto_lib_dir))
        .unique()
        .cloned()
        .collect();

    link_paths.iter().for_each(|p| println!("cargo:rustc-link-search=native={}", p.to_str().unwrap()));

    let include_paths: Vec<PathBuf> = pkg_configs
        .iter()
        .flat_map(|l| l.include_paths.iter())
        .chain(once(&spdk_include_dir))
        .chain(once(&spdk_module_dir))
        .unique()
        .cloned()
        .collect();

    let (static_libs, shared_libs): (Vec<_>, Vec<_>) = pkg_configs
        .iter()
        .flat_map(|l| l.libs.iter())
        .chain(once(&"isal".to_string()))
        .chain(once(&"isal_crypto".to_string()))
        .unique()
        .cloned()
        .partition(|l| link_paths.iter().any(|p| p.join(format!("lib{}.a", l)).exists()));

    static_libs
        .iter()
        .for_each(|l| println!("cargo:rustc-link-lib=static:+whole-archive,-bundle={}", l));
    shared_libs
        .iter()
        .for_each(|l| println!("cargo:rustc-link-lib=dylib={}", l));

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(DoxygenCallbacks::new()))
        .clang_args(include_paths.iter().map(|i| format!("-I{}", i.to_string_lossy().to_string())))
        .clang_args(defines.iter().map(|d| format!("-D{}", d)))
        .allowlist_function("spdk_.*")
        .allowlist_type("spdk_.*")
        .allowlist_var("spdk_.*")
        .allowlist_var("SPDK_.*")
        .opaque_type("spdk_nvme_(ctrlr|health|sgl|tcp)_.*")
        .opaque_type("spdk_nvmf_fabric_.*")
        .opaque_type("spdk_bdev_ext_io_opts")
        .wrap_static_fns(true)
        .wrap_static_fns_path(&spdk_wrappers)
        .wrap_unsafe_ops(true)
        .prepend_enum_name(false)
        .generate_cstr(true)
        .layout_tests(false);

    if include_bdev_module {
        builder = builder
            .clang_arg("-D CARGO_FEATURE_BDEV_MODULE=1")
    }

    if include_bdev_malloc {
        builder = builder
            .allowlist_function(".*_malloc_disk")
            .allowlist_type("malloc_bdev_opts");
    }

    if include_target_nvmf {
        builder = builder
            .allowlist_var("g_spdk_.*")
    }

    let _ = builder
        .generate()
        .expect("spdk bindings generated")
        .write_to_file(out_dir.join("bindings.rs").as_path());

    if spdk_wrappers.exists() {
        let mut wrappers = cc::Build::new();

        wrappers
            .file(spdk_wrappers)
            .include(".")
            .includes(include_paths);

        defines.iter().for_each(|d| _ = wrappers.define(d, None));

        wrappers
            .compile("spdk_wrappers");
    }
}
