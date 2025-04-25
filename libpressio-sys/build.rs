use std::{env, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio");
    println!("cargo::rerun-if-changed=std_compat");

    // ---------------------------------------------------------
    // Configure std_compat, the compiler portability layer
    // ---------------------------------------------------------
    let mut stdcompat_config = cmake::Config::new("std_compat");
    //prefer static libraries for RUST
    stdcompat_config.define("BUILD_SHARED_LIBS", "OFF");
    //disable testing to avoid google-test dependency
    stdcompat_config.define("BUILD_TESTING", "OFF");
    // require a C++17 compiler (e.g. gcc 12 or later) for now
    // this includes Ubuntu 24.04 and later, Fedora, Nyx, et al
    // https://robertu94.github.io/guides/dependencies
    stdcompat_config.define("STDCOMPAT_CXX_VERSION", "17");
    stdcompat_config.define("STDCOMPAT_CXX_UNSTABLE", "ON");
    stdcompat_config.define("STD_COMPAT_BOOST_REQUIRED", "OFF");
    let stdcompat_out = stdcompat_config.build();
    println!("cargo:rustc-link-search=native={}", stdcompat_out.display());

    // ---------------------------------------------------------
    // Configure libpressio
    // ---------------------------------------------------------
    let mut config = cmake::Config::new("libpressio");
    config.define("BUILD_SHARED_LIBS", "OFF");
    config.define("BUILD_TESTING", "OFF");
    config.define(
        "LIBPRESSIO_HAS_OPENMP",
        if cfg!(feature = "openmp") {
            "ON"
        } else {
            "OFF"
        },
    );
    config.define("CMAKE_PREFIX_PATH", stdcompat_out);
    let libpressio_out = config.build();

    println!(
        "cargo:rustc-link-search=native={}",
        libpressio_out.display()
    );
    println!("cargo:rustc-link-lib=static=:liblibpressio.a");
    eprintln!("include dir {}", libpressio_out.join("include").display());

    let cargo_callbacks = bindgen::CargoCallbacks::new();
    let bindings = bindgen::Builder::default()
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++11")
        .clang_arg(format!(
            "-I{}",
            libpressio_out.join("include").join("libpressio").display()
        ))
        .header("wrapper.h")
        .parse_callbacks(Box::new(cargo_callbacks))
        .allowlist_function("pressio_.*")
        .allowlist_var("pressio_.*")
        .allowlist_type("pressio_.*")
        // MSRV 1.85
        .rust_target(match bindgen::RustTarget::stable(85, 0) {
            Ok(target) => target,
            #[expect(clippy::panic)]
            Err(err) => panic!("{err}"),
        })
        .generate()
        .expect("Unable to generate bindings");

    let out_path =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set in a build script"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
