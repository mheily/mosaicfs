//! Reconciliation after reconnect.
//!
//! When CouchDB replication reconnects after an outage, run an expedited
//! full crawl using the mtime/size fast-path before resuming watch mode.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::time;
use tracing::{info, warn};

use crate::couchdb::CouchClient;

/// Monitor CouchDB replication state and trigger reconciliation on reconnect.
///
/// Returns a shutdown flag. Set it to `true` to stop monitoring.
pub fn start_reconnect_monitor(
    db: CouchClient,
    check_interval: Duration,
) -> Arc<AtomicBool> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    let needs_reconciliation = Arc::new(AtomicBool::new(false));

    tokio::spawn(async move {
        let mut was_connected = true;
        let mut interval = time::interval(check_interval);

        loop {
            interval.tick().await;

            if shutdown_clone.load(Ordering::Relaxed) {
                info!("Reconnect monitor shutting down");
                break;
            }

            let connected = check_couchdb_available(&db).await;

            if !was_connected && connected {
                info!("CouchDB connection restored, reconciliation needed");
                needs_reconciliation.store(true, Ordering::Relaxed);
            } else if was_connected && !connected {
                warn!("CouchDB connection lost");
            }

            was_connected = connected;
        }
    });

    shutdown
}

/// Check if CouchDB is reachable.
async fn check_couchdb_available(db: &CouchClient) -> bool {
    let url = format!("{}/", db.base_url);
    match db
        .client
        .get(&url)
        .basic_auth(&db.auth.0, Some(&db.auth.1))
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Run an expedited reconciliation crawl.
/// This is a thin wrapper that signals the agent to re-crawl.
/// The actual crawl logic lives in mosaicfs-agent::crawler.
///
/// Returns `true` if reconciliation was triggered.
pub async fn check_and_reconcile(
    needs_reconciliation: &AtomicBool,
) -> bool {
    if needs_reconciliation.load(Ordering::Relaxed) {
        needs_reconciliation.store(false, Ordering::Relaxed);
        info!("Running reconciliation crawl after reconnect");
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_reconciliation_flag() {
        let flag = AtomicBool::new(false);
        assert!(!flag.load(Ordering::Relaxed));

        flag.store(true, Ordering::Relaxed);
        assert!(flag.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_check_and_reconcile() {
        let flag = AtomicBool::new(true);
        assert!(check_and_reconcile(&flag).await);
        // After reconciliation, flag is cleared
        assert!(!check_and_reconcile(&flag).await);
    }
}
