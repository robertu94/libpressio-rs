use std::env;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=mpi-stubs");

    // ---------------------------------------------------------
    // Configure mpi-sys, the serial MPI implementation
    // ---------------------------------------------------------
    let mut config = cmake::Config::new("mpi-stubs");
    configure_cmake_tools(&mut config);
    // prefer static libraries for Rust
    config.define("BUILD_SHARED_LIBS", "OFF");
    let mpi_stubs_out = config.build();

    println!(
        "cargo::rustc-link-search=native={}",
        mpi_stubs_out.display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        mpi_stubs_out.join("lib").display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        mpi_stubs_out.join("lib64").display()
    );
    println!("cargo::rustc-link-lib=static=mpi");

    println!("cargo::metadata=root={}", mpi_stubs_out.display());
    println!(
        "cargo::metadata=include={}",
        mpi_stubs_out.join("include").display()
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
