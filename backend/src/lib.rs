//! Library surface for the TickerTapeTallyBoard backend.
//!
//! `main.rs` is a thin wrapper over this crate; `examples/` and `tests/`
//! reuse these modules instead of duplicating logic.

pub mod api;
pub mod app;
pub mod config;
pub mod db;
pub mod domain;
pub mod engine_logging;
pub mod import;
pub mod providers;
pub mod state;
