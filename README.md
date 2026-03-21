[![CI Status]][workflow] [![MSRV]][repo] [![Latest Version]][crates.io] [![Rust Doc Crate]][docs.rs] [![Rust Doc Main]][docs]

[CI Status]: https://img.shields.io/github/actions/workflow/status/robertu94/libpressio-rs/ci.yml?branch=main
[workflow]: https://github.com/robertu94/libpressio-r/actions/workflows/ci.yml?query=branch%3Amain

[MSRV]: https://img.shields.io/badge/MSRV-1.85.0-blue
[repo]: https://github.com/robertu94/libpressio-r

[Latest Version]: https://img.shields.io/crates/v/libpressio
[crates.io]: https://crates.io/crates/libpressio

[Rust Doc Crate]: https://img.shields.io/docsrs/libpressio
[docs.rs]: https://docs.rs/libpressio/

[Rust Doc Main]: https://img.shields.io/badge/docs-main-blue
[docs]: https://robertu94.github.io/libpressio-rs/libpressio

# libpressio

High-level Rust bindigs to the [libpressio] compression framework.

[libpressio]: https://github.com/robertu94/libpressio

## Features

This crate has the following features:
- `bzip2`: enables the bzip2 compressor
- `lua`: enables the Lua-based lambda function compressor and metrics scripts, currently no support for LuaJit is provided
- `openmp`: enables OpenMP support using pre-installed OpenMP

## License

Licensed under the OPEN SOURCE LICENSE (license number: SF-19-112), see [COPYRIGHT.txt](COPYRIGHT.txt).

## Funding

Some upgrades to the `libpressio` crate have been developed as part of [ESiWACE3](https://www.esiwace.eu), the third phase of the Centre of Excellence in Simulation of Weather and Climate in Europe.

Funded by the European Union. This work has received funding from the European High Performance Computing Joint Undertaking (JU) under grant agreement No 101093054.
