use std::env;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=std_compat");

    // ---------------------------------------------------------
    // Configure std_compat, the compiler portability layer
    // ---------------------------------------------------------
    let mut stdcompat_config = cmake::Config::new("std_compat");
    configure_cmake_tools(&mut stdcompat_config);
    // prefer static libraries for Rust
    stdcompat_config.define("BUILD_SHARED_LIBS", "OFF");
    // disable testing to avoid google-test dependency
    stdcompat_config.define("BUILD_TESTING", "OFF");
    // require a C++17 compiler (e.g. gcc 12 or later) for now
    // this includes Ubuntu 24.04 and later, Fedora, Nyx, et al
    // https://robertu94.github.io/guides/dependencies
    stdcompat_config.define("STDCOMPAT_CXX_VERSION", "17");
    stdcompat_config.define("STDCOMPAT_CXX_UNSTABLE", "ON");
    stdcompat_config.define("STD_COMPAT_BOOST_REQUIRED", "OFF");
    let stdcompat_out = stdcompat_config.build();

    println!(
        "cargo::rustc-link-search=native={}",
        stdcompat_out.display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        stdcompat_out.join("lib").display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        stdcompat_out.join("lib64").display()
    );
    println!("cargo::rustc-link-lib=static=std_compat");

    println!("cargo::metadata=root={}", stdcompat_out.display());
    println!(
        "cargo::metadata=include={}",
        stdcompat_out.join("include").display()
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
