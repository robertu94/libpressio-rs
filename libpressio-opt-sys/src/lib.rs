#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(feature = "mpi-stubs")]
// ensure that mpi-stubs-sys is linked
extern crate mpi_stubs_sys as _;

// ensure that libdistributed-sys is linked
extern crate libdistributed_sys as _;
