use std::{
    env,
    fs::canonicalize,
    path::PathBuf,
};

use fs_extra::dir;
use itertools::Itertools;

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

    if !spdk_dir.exists() {
        let copy_options = dir::CopyOptions::new()
            .overwrite(true);

        dir::copy(spdk_src_dir, out_dir.clone(), &copy_options).expect("$OUT_DIR is writeable");
    }

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

    let include_bdev_malloc = env::var_os("CARGO_FEATURE_BDEV_MALLOC").is_some();

    if include_bdev_malloc {
        pkg_configs.push(pkg_config.probe("spdk_bdev_malloc").expect("spdk_bdev_malloc package config exists"));
        pkg_configs.push(pkg_config.probe("spdk_event_bdev").expect("spdk_event_bdev package config exists"));
    }

    let link_paths: Vec<PathBuf> = pkg_configs
        .iter()
        .flat_map(|l| l.link_paths.iter())
        .unique()
        .cloned()
        .collect();

    link_paths.iter().for_each(|p| println!("cargo:rustc-link-search=native={}", p.to_str().unwrap()));

    let (static_libs, shared_libs): (Vec<_>, Vec<_>) = pkg_configs
        .iter()
        .flat_map(|l| l.libs.iter())
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
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .clang_args(&[
            "-I", spdk_include_dir.to_string_lossy().as_ref(),
            "-I", spdk_module_dir.to_string_lossy().as_ref(),
            ])
        .allowlist_function("spdk_.*")
        .allowlist_type("spdk_.*")
        .allowlist_var("spdk_.*")
        .opaque_type("spdk_nvme_(sgl_descriptor|ctrlr_data|health_information_page)")
        .wrap_unsafe_ops(true)
        .prepend_enum_name(false)
        .layout_tests(false);

    if include_bdev_malloc {
        builder = builder
            .clang_arg("-D CARGO_FEATURE_BDEV_MALLOC=1")
            .allowlist_function(".*_malloc_disk")
            .allowlist_type("malloc_bdev_opts");
    }

    let _ = builder
        .generate()
        .expect("spdk bindings generated")
        .write_to_file(out_dir.join("bindings.rs").as_path());
}
