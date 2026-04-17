//! Agent subsystem for MosaicFS.
//!
//! Runs the filesystem crawler, replication runner, heartbeat publisher,
//! and node-level health checks. Normally started from the unified
//! `mosaicfs` binary via [`start_agent`] when `features.agent = true`.

pub mod backend;
pub mod crawler;
pub mod node;
pub mod replication;
pub mod replication_subsystem;

mod start;

pub use start::start_agent;
