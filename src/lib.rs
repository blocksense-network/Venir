#![cfg_attr(test, feature(rustc_private))]

// The lib test harness links rust_verify, which links rustc_driver; without
// this the test binary fails with "cannot satisfy dependencies so `rustc_driver` only
// shows up once".
#[cfg(test)]
extern crate rustc_driver;

pub mod vstd_utils;
pub mod vir_optimizers;
pub mod consts;
pub mod verify_crate;
pub mod stub_structs;