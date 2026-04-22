#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(feature = "bzip2")]
// ensure that bzip2-sys is linked
extern crate bzip2_sys as _;

#[cfg(feature = "lua")]
// ensure that mlua-sys is linked
extern crate mlua_sys as _;
