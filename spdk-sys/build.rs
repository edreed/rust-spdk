use std::{env, path::PathBuf, fs::canonicalize};

use fs_extra::dir;
use itertools::{self, chain, Itertools};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");

    let spdk_src_dir = canonicalize("spdk").expect("spdk submodule to be initialized");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("$OUT_DIR set by build"));
    let spdk_dir = out_dir.join("spdk");
    let spdk_lib_dir = spdk_dir.join("build/lib");
    let spdk_pkgconfig_dir = spdk_lib_dir.join("pkgconfig");
    let spdk_include_dir = spdk_src_dir.join("include");

    if !spdk_dir.exists() {
        let copy_options = dir::CopyOptions::new()
            .overwrite(true);

        dir::copy(spdk_src_dir, out_dir.clone(), &copy_options).expect("$OUT_DIR is writeable");
    }

    let _dst = autotools::Config::new(spdk_dir)
        .forbid("--disable-shared")
        .forbid("--enable-static")
        .disable("apps", None)
        .disable("examples", None)
        .disable("tests", None)
        .disable("unit-tests", None)
        .config_option("prefix", Some(""))
        .insource(true)
        .make_target("all")
        .build();

    let old_pkg_config_path = env::var("PKG_CONFIG_PATH").unwrap_or("".into());

    env::set_var("PKG_CONFIG_PATH", format!("{}:{}", old_pkg_config_path, spdk_pkgconfig_dir.to_str().unwrap()));

    let mut pkg_config = pkg_config::Config::new();
    pkg_config
        .cargo_metadata(false)
        .env_metadata(false)
        .statik(true);

    let spdk_event_config = pkg_config.probe("spdk_event").expect("spdk_event package config exists");
    let spdk_env_dpdk_config = pkg_config.probe("spdk_env_dpdk").expect("spdk_env_dpdk package config exists");

    chain!(spdk_event_config.link_paths, spdk_env_dpdk_config.link_paths)
        .unique()
        .for_each(|p| println!("cargo:rustc-link-search=native={}", p.to_str().unwrap()));

    chain!(spdk_event_config.libs, spdk_env_dpdk_config.libs)
        .unique()
        .for_each(|l| println!("cargo:rustc-link-lib=static:+whole-archive,-bundle={}", l));

    println!("cargo:rustc-link-lib=dylib=numa");

    let _bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .clang_args(&["-I", spdk_include_dir.to_string_lossy().as_ref()])
        .allowlist_function("spdk_.*")
        .allowlist_type("spdk_.*")
        .allowlist_var("spdk_.*")
        .wrap_unsafe_ops(true)
        .prepend_enum_name(false)
        .layout_tests(false)
        .generate()
        .expect("spdk bindings generated")
        .write_to_file(out_dir.join("bindings.rs").as_path());
}
