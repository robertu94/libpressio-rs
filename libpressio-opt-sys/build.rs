use std::{env, ffi::OsString, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio_opt");

    let out_dir = env::var("OUT_DIR")
        .map(PathBuf::from)
        .expect("missing OUT_DIR");

    let mut cmake_prefix_path = OsString::new();

    let std_compat_root = env::var("DEP_STD_COMPAT_ROOT")
        .map(PathBuf::from)
        .expect("missing std_compat dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(std_compat_root);

    let libdistributed_root = env::var("DEP_LIBDISTRIBUTED_ROOT")
        .map(PathBuf::from)
        .expect("missing libdistributed dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(libdistributed_root);

    let libpressio_root = env::var("DEP_LIBPRESSIO_ROOT")
        .map(PathBuf::from)
        .expect("missing libpressio dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(libpressio_root);
    let libpressio_prefix = env::var("DEP_LIBPRESSIO_PREFIX")
        .map(PathBuf::from)
        .expect("missing libpressio dependency");
    cmake_prefix_path.push(";");
    cmake_prefix_path.push(libpressio_prefix);

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
    let libpressio_opt_out = config.build();

    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_opt_out.display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_opt_out.join("lib").display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_opt_out.join("lib64").display()
    );
    println!("cargo::rustc-link-lib=static=libpressio_opt");

    println!("cargo::metadata=root={}", libpressio_opt_out.display());
    println!(
        "cargo::metadata=include={}",
        libpressio_opt_out.join("include").display()
    );

    let cargo_callbacks = CargoCallBacksIngoreGeneratedFiles::new(["pressio_version.h"]);
    let bindings = bindgen::Builder::default()
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++17")
        .clang_arg(format!(
            "-I{}",
            libpressio_opt_out
                .join("include")
                .join("libpressio_opt")
                .display()
        ))
        .header("wrapper.h")
        .parse_callbacks(Box::new(cargo_callbacks))
        .allowlist_function("libpressio_register_libpressio_opt")
        .derive_copy(false)
        .derive_debug(false)
        .derive_default(false)
        .derive_eq(false)
        .derive_hash(false)
        .derive_ord(false)
        .derive_ord(false)
        .derive_partialeq(false)
        .derive_partialord(false)
        // MSRV 1.85: must match the workspace rust-version
        .rust_target(match bindgen::RustTarget::stable(85, 0) {
            Ok(target) => target,
            #[expect(clippy::panic)]
            Err(err) => panic!("{err}"),
        })
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

#[derive(Debug)]
struct CargoCallBacksIngoreGeneratedFiles {
    cargo_callbacks: bindgen::CargoCallbacks,
    files: std::vec::Vec<regex::Regex>,
}

impl CargoCallBacksIngoreGeneratedFiles {
    fn new<'a, T>(files: T) -> Self
    where
        T: IntoIterator<Item = &'a str>,
    {
        Self {
            cargo_callbacks: bindgen::CargoCallbacks::new(),
            files: Vec::from_iter(files.into_iter().map(|v| regex::Regex::new(v).unwrap())),
        }
    }
}

impl bindgen::callbacks::ParseCallbacks for CargoCallBacksIngoreGeneratedFiles {
    fn header_file(&self, filename: &str) {
        if !self.files.iter().any(|f| f.is_match(filename)) {
            self.cargo_callbacks.header_file(filename)
        }
    }
    fn include_file(&self, filename: &str) {
        if !self.files.iter().any(|f| f.is_match(filename)) {
            self.cargo_callbacks.include_file(filename)
        }
    }
    fn read_env_var(&self, filename: &str) {
        self.cargo_callbacks.read_env_var(filename)
    }
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
