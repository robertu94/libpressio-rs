use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-changed=libpressio");

    let mut config = cmake::Config::new("libpressio");
    let out = config.build();

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=libpressio");

    let cargo_callbacks = bindgen::CargoCallbacks::new();
    let bindings = bindgen::Builder::default()
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++11")
        .clang_arg(format!(
            "-I{}",
            Path::new("libpressio").join("include").display()
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
