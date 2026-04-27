//! In-process agent subsystem.
//!
//! Starts the mosaicfs crawler and replication subsystem as a background
//! tokio task. The agent runs until the process receives SIGTERM (app quit).
//!
//! # Limitations
//!
//! * `watch_paths` / `excluded_paths` changes require an app restart — the
//!   agent task has no cancellation path, so settings saved via the UI do not
//!   propagate. See docs/changes/014/discussion.md.
//!
//! * The agent is not started when `settings.watch_paths` is empty.

use std::path::Path;
use std::sync::Arc;

use mosaicfs_common::config::{
    AgentFeatureConfig, CouchdbConfig, FeaturesConfig, MosaicfsConfig, NodeConfig, SecretsConfig,
};
use mosaicfs_common::secrets::InlineBackend;

use crate::settings::Settings;

/// Spawn the agent as a background task if `watch_paths` is non-empty.
/// `node_id` should be the value already resolved by `server::build_router`
/// so both subsystems share the same node identity.
pub fn start(
    settings: &Settings,
    app_data_dir: &Path,
    node_id: Option<String>,
    provider: Arc<dyn mosaicfs_agent::WatchPathProvider>,
) {
    if settings.watch_paths.is_empty() {
        return;
    }

    let watch_paths = settings.watch_paths.iter().map(Into::into).collect();
    let excluded_paths = settings.excluded_paths.iter().map(Into::into).collect();
    let state_dir = app_data_dir.join("agent-state");

    let cfg = Arc::new(MosaicfsConfig {
        node: NodeConfig { node_id },
        features: FeaturesConfig {
            agent: true,
            vfs: false,
            web_ui: false,
        },
        couchdb: CouchdbConfig {
            url: settings.couchdb_url.clone(),
            user: settings.couchdb_user.clone(),
            password: settings.couchdb_password.clone(),
        },
        agent: Some(AgentFeatureConfig {
            watch_paths,
            excluded_paths,
            state_dir: Some(state_dir),
        }),
        vfs: None,
        web_ui: None,
        credentials: None,
        secrets: SecretsConfig::default(),
    });

    let secrets = Arc::new(InlineBackend::from_config(&cfg, None));

    tauri::async_runtime::spawn(async move {
        if let Err(e) = mosaicfs_agent::start_agent(cfg, secrets, provider).await {
            eprintln!("mosaicfs-desktop: agent exited with error: {e}");
        }
    });
}
