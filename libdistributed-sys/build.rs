use std::{env, ffi::OsString, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=libdistributed");

    let std_compat_root = env::var("DEP_STD_COMPAT_ROOT")
        .map(PathBuf::from)
        .expect("missing std_compat dependency");

    let cmake_prefix_path = OsString::from(std_compat_root);

    // ---------------------------------------------------------
    // Configure libdistributed, the MPI facilities
    // ---------------------------------------------------------
    let mut config = cmake::Config::new("libdistributed");
    configure_cmake_tools(&mut config);
    // prefer static libraries for Rust
    config.define("BUILD_SHARED_LIBS", "OFF");
    // disable testing
    config.define("BUILD_TESTING", "OFF");

    if cfg!(feature = "mpi-stubs") {
        let mpi_stubs_root = env::var("DEP_MPI_STUBS_ROOT")
            .map(PathBuf::from)
            .expect("missing mpi-stubs dependency");
        config.define("MPI_CXX_HEADER_DIR", mpi_stubs_root.join("include"));
        config.define("MPI_CXX_LIB_NAMES", "mpi");
        config.define(
            "MPI_mpi_LIBRARY",
            mpi_stubs_root.join("lib").join("libmpi.a"),
        );
    }

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
    println!("cargo::rustc-link-lib=static=libdistributed");

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
