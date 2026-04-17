//! Shared crate surface for both binaries.
//! `main.rs` uses the TUI modules, while `bin/web_server.rs` reuses feed and model code.

pub mod adapters;
pub mod demo;
pub mod f1;
pub mod favourites;
pub mod imsa;
pub mod nls;
pub mod timing;
pub mod ui;
pub mod web;
