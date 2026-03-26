use std::{env, ffi::OsString, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=libdistributed");

    let mut cmake_prefix_path = OsString::new();

    let std_compat_root = env::var("DEP_STD_COMPAT_ROOT")
        .map(PathBuf::from)
        .expect("missing std_compat dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(std_compat_root);

    let distributed_root = env::var("DEP_DISTRIBUTED_ROOT")
        .map(PathBuf::from)
        .expect("missing distributed dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(distributed_root);

    let pressio_root = env::var("DEP_PRESSIO_ROOT")
        .map(PathBuf::from)
        .expect("missing pressio dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(pressio_root);

    // ---------------------------------------------------------
    // Configure libpressio_opt, the autotuning plugin for libpressio
    // ---------------------------------------------------------
    let mut config = cmake::Config::new("libpressio_opt");
    configure_cmake_tools(&mut config);
    // prefer static libraries for Rust
    config.define("BUILD_SHARED_LIBS", "OFF");
    // disable testing
    config.define("BUILD_TESTING", "OFF");
    // disable fraz support
    config.define("LIBPRESSIO_OPT_HAS_DLIB", "OFF");
    config.define("CMAKE_PREFIX_PATH", cmake_prefix_path);
    let libdistributed_out = config.build();

    println!(
        "cargo::rustc-link-search=native={}",
        libdistributed_out.display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libdistributed_out.join("lib").display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libdistributed_out.join("lib64").display()
    );
    println!("cargo::rustc-link-lib=static=std_compat");

    println!("cargo::metadata=root={}", libdistributed_out.display());
    println!(
        "cargo::metadata=include={}",
        libdistributed_out.join("include").display()
    );
}

fn configure_cmake_tools(config: &mut cmake::Config) {
    if let Ok(ar) = env::var("AR") {
        config.define("CMAKE_AR", ar);
    }
    if let Ok(ld) = env::var("LD") {
        config.define("CMAKE_LINKER", ld);
    }
    if let Ok(nm) = env::var("NM") {
        config.define("CMAKE_NM", nm);
    }
    if let Ok(objdump) = env::var("OBJDUMP") {
        config.define("CMAKE_OBJDUMP", objdump);
    }
    if let Ok(ranlib) = env::var("RANLIB") {
        config.define("CMAKE_RANLIB", ranlib);
    }
    if let Ok(strip) = env::var("STRIP") {
        config.define("CMAKE_STRIP", strip);
    }
}
