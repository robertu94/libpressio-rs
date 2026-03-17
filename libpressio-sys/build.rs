use std::{env, ffi::OsString, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio");
    println!("cargo::rerun-if-changed=std_compat");

    let out_dir = env::var("OUT_DIR")
        .map(PathBuf::from)
        .expect("missing OUT_DIR");

    let target = env::var("TARGET").expect("missing TARGET");

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

    let sol2_out = if cfg!(feature = "lua") {
        let lua_root = env::var("DEP_LUA_ROOT")
            .map(PathBuf::from)
            .expect("missing lua dependency");
        // ---------------------------------------------------------
        // Configure sol2
        // ---------------------------------------------------------
        let mut sol2_config = cmake::Config::new("sol2");
        configure_cmake_tools(&mut sol2_config);
        sol2_config.define("SOL2_ENABLE_INSTALL", "ON");
        sol2_config.define("SOL2_BUILD_LUA", "OFF");
        sol2_config.define("SOL2_LUA_VERSION", "5.4");
        sol2_config.define("CMAKE_PREFIX_PATH", lua_root);
        Some(sol2_config.build())
    } else {
        None
    };

    // ---------------------------------------------------------
    // Configure libpressio
    // ---------------------------------------------------------
    let mut libpressio_config = cmake::Config::new("libpressio");
    configure_cmake_tools(&mut libpressio_config);
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

    let mut libpressio_cmake_prefix_path = OsString::from(stdcompat_out);
    if cfg!(feature = "bzip2") {
        let bzip2_root = env::var("DEP_BZIP2_ROOT")
            .map(PathBuf::from)
            .expect("missing bzip2 dependency");
        libpressio_cmake_prefix_path.push(";");
        libpressio_cmake_prefix_path.push(bzip2_root);
        libpressio_config.define("LIBPRESSIO_HAS_BZIP2", "ON");
    } else {
        libpressio_config.define("LIBPRESSIO_HAS_BZIP2", "OFF");
    }
    if let Some(sol2_out) = sol2_out {
        libpressio_cmake_prefix_path.push(";");
        libpressio_cmake_prefix_path.push(sol2_out);
        let lua_root = env::var("DEP_LUA_ROOT")
            .map(PathBuf::from)
            .expect("missing lua dependency");
        libpressio_cmake_prefix_path.push(";");
        libpressio_cmake_prefix_path.push(lua_root);
        libpressio_config.define("LIBPRESSIO_HAS_LUA", "ON");
    } else {
        libpressio_config.define("LIBPRESSIO_HAS_LUA", "OFF");
    }
    libpressio_config.define("CMAKE_PREFIX_PATH", libpressio_cmake_prefix_path);

    libpressio_config.define("LIBPRESSIO_BUILD_MODE", "FULL");
    libpressio_config.define(
        "LIBPRESSIO_WITH_EXTERNAL",
        if target.contains("wasm") { "OFF" } else { "ON" },
    );
    let libpressio_out = libpressio_config.build();

    if target.contains("-linux-") || target.ends_with("-linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else if target.ends_with("-darwin") {
        println!("cargo:rustc-link-lib=dylib=c++");
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
        .derive_copy(false)
        .derive_debug(false)
        .derive_default(false)
        .derive_eq(false)
        .derive_hash(false)
        .derive_ord(false)
        .derive_ord(false)
        .derive_partialeq(false)
        .derive_partialord(false)
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
