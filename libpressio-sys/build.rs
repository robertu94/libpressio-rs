use std::{env, path::PathBuf};

#[derive(Debug)]
struct CargoCallBacksIngoreGeneratedFiles {
    cargo_callbacks: bindgen::CargoCallbacks,
    files: std::vec::Vec<regex::Regex>,
}
impl CargoCallBacksIngoreGeneratedFiles {
    fn new<'a, T>(files: T) -> Result<CargoCallBacksIngoreGeneratedFiles, anyhow::Error>
    where
        T: IntoIterator<Item = &'a str>,
    {
        Ok(CargoCallBacksIngoreGeneratedFiles {
            cargo_callbacks: bindgen::CargoCallbacks::new(),
            files: Vec::from_iter(files.into_iter().map(|v| regex::Regex::new(v).unwrap())),
        })
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio");
    println!("cargo::rerun-if-changed=std_compat");

    let out_dir = env::var("OUT_DIR")
        .map(PathBuf::from)
        .expect("missing OUT_DIR");

    let target = env::var("TARGET").expect("missing TARGET");
    let target_os = target.split('-').nth(2).expect("invalid TARGET triple");

    // ---------------------------------------------------------
    // Configure std_compat, the compiler portability layer
    // ---------------------------------------------------------
    let mut stdcompat_config = cmake::Config::new("std_compat");
    if let Ok(ar) = env::var("AR") {
        stdcompat_config.define("CMAKE_AR", ar);
    }
    if let Ok(ld) = env::var("LD") {
        stdcompat_config.define("CMAKE_LINKER", ld);
    }
    if let Ok(nm) = env::var("NM") {
        stdcompat_config.define("CMAKE_NM", nm);
    }
    if let Ok(objdump) = env::var("OBJDUMP") {
        stdcompat_config.define("CMAKE_OBJDUMP", objdump);
    }
    if let Ok(ranlib) = env::var("RANLIB") {
        stdcompat_config.define("CMAKE_RANLIB", ranlib);
    }
    if let Ok(strip) = env::var("STRIP") {
        stdcompat_config.define("CMAKE_STRIP", strip);
    }
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
    println!(
        "cargo::rustc-link-search=native={}",
        stdcompat_out.display()
    );

    // ---------------------------------------------------------
    // Configure libpressio
    // ---------------------------------------------------------
    let mut libpressio_config = cmake::Config::new("libpressio");
    if let Ok(ar) = env::var("AR") {
        libpressio_config.define("CMAKE_AR", ar);
    }
    if let Ok(ld) = env::var("LD") {
        libpressio_config.define("CMAKE_LINKER", ld);
    }
    if let Ok(nm) = env::var("NM") {
        libpressio_config.define("CMAKE_NM", nm);
    }
    if let Ok(objdump) = env::var("OBJDUMP") {
        libpressio_config.define("CMAKE_OBJDUMP", objdump);
    }
    if let Ok(ranlib) = env::var("RANLIB") {
        libpressio_config.define("CMAKE_RANLIB", ranlib);
    }
    if let Ok(strip) = env::var("STRIP") {
        libpressio_config.define("CMAKE_STRIP", strip);
    }
    libpressio_config.define("BUILD_SHARED_LIBS", "OFF");
    libpressio_config.define("BUILD_TESTING", "OFF");
    libpressio_config.define(
        "LIBPRESSIO_HAS_OPENMP",
        if cfg!(feature = "openmp") {
            "ON"
        } else {
            "OFF"
        },
    );
    libpressio_config.define("CMAKE_PREFIX_PATH", stdcompat_out);
    let libpressio_out = libpressio_config.build();

    println!("cargo:rustc-link-lib=static=libpressio");
    if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_out.display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_out.join("lib").display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        libpressio_out.join("lib64").display()
    );
    println!("cargo::rustc-link-lib=static=libpressio");

    let cargo_callbacks = CargoCallBacksIngoreGeneratedFiles::new(["pressio_version.h"])?;
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

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
    Ok(())
}
