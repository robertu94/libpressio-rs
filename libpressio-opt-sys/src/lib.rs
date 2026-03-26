#[cfg(feature = "mpi-stubs")]
// ensure that mpi-stubs-sys is linked
extern crate mpi_stubs_sys as _;

// ensure that libdistributed-sys is linked
extern crate libdistributed_sys as _;
