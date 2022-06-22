extern crate bindgen;
extern crate pkg_config;

use std::env;
use std::path::PathBuf;

pub fn main() {
    println!("cargo:rustc-link-lib=libpressio");
    println!("cargo:rerun-if-changed=./build/wrapper.h");
    println!("cargo:rerun-if-changed=./build.rs");

    let libpressio = pkg_config::probe_library("libpressio").unwrap();
    let include_flag = format!("-I{}", libpressio.include_paths[0].to_str().unwrap());
    //println!("{}", include_flag);

    let bindings = bindgen::Builder::default()
        .header("./build/wrapper.h")
        .clang_arg(include_flag)
        .whitelist_function("pressio_.*")
        .whitelist_var("pressio_.*")
        .whitelist_type("pressio_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("unable to generate bindings");
    let output = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(output.join("bindings.rs"))
        .expect("couldn't write bindings");
}
