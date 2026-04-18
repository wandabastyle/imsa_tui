//! Shared crate surface for both binaries.
//! `main.rs` uses the TUI modules, while `bin/web_server.rs` reuses feed and model code.

pub mod adapters;
pub mod demo;
pub mod favourites;
pub mod feed;
pub mod snapshot_runtime;
pub mod timing;
pub mod timing_persist;
pub mod ui;
pub mod web;
