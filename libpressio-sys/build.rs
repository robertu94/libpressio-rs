use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio");

    let out_dir = env::var("OUT_DIR")
        .map(PathBuf::from)
        .expect("missing OUT_DIR");

    let target = env::var("TARGET").expect("missing TARGET");

    let std_compat_root = env::var("DEP_STD_COMPAT_ROOT")
        .map(PathBuf::from)
        .expect("missing std_compat dependency");

    // ---------------------------------------------------------
    // Configure libpressio
    // ---------------------------------------------------------
    let mut config = cmake::Config::new("libpressio");
    configure_cmake_tools(&mut config);
    config.define("BUILD_SHARED_LIBS", "OFF");
    config.define("BUILD_TESTING", "OFF");

    if cfg!(feature = "openmp") {
        let openmp_flag = env::var("DEP_OPENMP_FLAG").expect("missing OpenMP flag");
        for f in openmp_flag.split(' ') {
            config.cflag(f);
            config.cxxflag(f);
        }
        config.define("LIBPRESSIO_HAS_OPENMP", "ON");
    } else {
        config.define("LIBPRESSIO_HAS_OPENMP", "OFF");
    }

    let mut cmake_prefix_path = OsString::from(std_compat_root);

    if cfg!(feature = "bzip2") {
        let bzip2_root = env::var("DEP_BZIP2_ROOT")
            .map(PathBuf::from)
            .expect("missing bzip2 dependency");
        cmake_prefix_path.push(";");
        cmake_prefix_path.push(bzip2_root);
        config.define("LIBPRESSIO_HAS_BZIP2", "ON");
    } else {
        config.define("LIBPRESSIO_HAS_BZIP2", "OFF");
    }

    if cfg!(feature = "lua") {
        let sol2_root = env::var("DEP_SOL2_ROOT")
            .map(PathBuf::from)
            .expect("missing sol2 dependency");
        cmake_prefix_path.push(";");
        cmake_prefix_path.push(sol2_root);
        let lua_root = env::var("DEP_LUA_ROOT")
            .map(PathBuf::from)
            .expect("missing lua dependency");
        cmake_prefix_path.push(";");
        cmake_prefix_path.push(lua_root);
        config.define("LIBPRESSIO_HAS_LUA", "ON");
    } else {
        config.define("LIBPRESSIO_HAS_LUA", "OFF");
    }

    if cfg!(feature = "distributed") {
        let libdistributed_root = env::var("DEP_LIBDISTRIBUTED_ROOT")
            .map(PathBuf::from)
            .expect("missing libdistributed dependency");
        cmake_prefix_path.push(";");
        cmake_prefix_path.push(libdistributed_root);
        config.define("LIBPRESSIO_HAS_LIBDISTRIBUTED", "ON");
    } else {
        config.define("LIBPRESSIO_HAS_LIBDISTRIBUTED", "OFF");
    }

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

    println!(
        "cargo::metadata=prefix={}",
        Path::new(&cmake_prefix_path).display()
    );
    config.define("CMAKE_PREFIX_PATH", cmake_prefix_path);

    config.define("LIBPRESSIO_BUILD_MODE", "FULL");
    config.define(
        "LIBPRESSIO_WITH_EXTERNAL",
        if target.contains("wasm") { "OFF" } else { "ON" },
    );
    let libpressio_out = config.build();

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

    println!("cargo::metadata=root={}", libpressio_out.display());
    println!(
        "cargo::metadata=include={}",
        libpressio_out.join("include").display()
    );

    if cfg!(feature = "openmp") {
        if let Some(links) = env::var_os("DEP_OPENMP_CARGO_LINK_INSTRUCTIONS") {
            for link in env::split_paths(&links) {
                if !link.as_os_str().is_empty() {
                    println!("cargo::{}", link.display());
                }
            }
        }
    }

    let cargo_callbacks = CargoCallBacksIngoreGeneratedFiles::new(["pressio_version.h"])?;
    let bindings = bindgen::Builder::default()
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++17")
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
