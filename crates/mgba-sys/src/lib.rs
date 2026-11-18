//! Raw FFI bindings to libmgba 0.10.5, linked statically. libmgba is MPL-2.0: it is
//! built unmodified from the pinned upstream tag recorded in `toolchain/sources.json`.
//! Only `tauros-emu` may depend on this crate; everything else uses its safe API.
#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
