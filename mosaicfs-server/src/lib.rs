//! Web UI subsystem for MosaicFS.
//!
//! Exposes the admin UI and REST API. Normally started from the unified
//! `mosaicfs` binary via [`start_web_ui`] when `features.web_ui = true`.

pub mod access_cache;
pub mod ui;
pub mod auth;
pub mod credentials;
pub mod handlers;
pub mod label_cache;
pub mod readdir;
pub mod readdir_cache;
pub mod routes;
pub mod state;
pub mod tls;

mod start;

pub use start::{build_app_router, run_bootstrap, start_web_ui};
