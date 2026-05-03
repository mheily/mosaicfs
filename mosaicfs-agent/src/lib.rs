//! Agent subsystem for MosaicFS.
//!
//! Runs the filesystem crawler, heartbeat publisher,
//! and node-level health checks. Normally started from the unified
//! `mosaicfs` binary via [`start_agent`] when `features.agent = true`.

pub mod crawler;
pub mod node;
pub mod watch_path_provider;

mod start;

pub use start::start_agent;
pub use watch_path_provider::{BareWatchPathProvider, OpenedWatchPath, WatchPathProvider};
