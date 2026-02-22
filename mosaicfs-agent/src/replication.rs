use tracing::{info, warn};

use crate::couchdb::CouchClient;

/// Set up bidirectional continuous replication between local and control plane CouchDB.
pub async fn setup_replication(
    local_db: &CouchClient,
    control_plane_url: &str,
    db_name: &str,
    remote_user: &str,
    remote_password: &str,
) -> anyhow::Result<()> {
    let remote_url = format!(
        "{}://{}:{}@{}/{}",
        parse_scheme(control_plane_url),
        urlencoding::encode(remote_user),
        urlencoding::encode(remote_password),
        parse_host(control_plane_url),
        db_name,
    );

    // Push replication (local → control plane)
    let push_doc = serde_json::json!({
        "_id": format!("mosaicfs-push-{}", db_name),
        "source": db_name,
        "target": remote_url,
        "continuous": true,
        "create_target": true,
    });

    // Pull replication (control plane → local)
    let pull_doc = serde_json::json!({
        "_id": format!("mosaicfs-pull-{}", db_name),
        "source": remote_url,
        "target": db_name,
        "continuous": true,
    });

    // Write to _replicator database
    let replicator = CouchClient::new(
        &local_db.base_url(),
        "_replicator",
        &local_db.auth().0,
        &local_db.auth().1,
    );
    replicator.ensure_db().await?;

    match replicator.put_document(&push_doc["_id"].as_str().unwrap(), &push_doc).await {
        Ok(_) => info!("Push replication configured"),
        Err(crate::couchdb::CouchError::Conflict(_)) => {
            info!("Push replication already configured");
        }
        Err(e) => {
            warn!(error = %e, "Failed to configure push replication");
            return Err(e.into());
        }
    }

    match replicator.put_document(&pull_doc["_id"].as_str().unwrap(), &pull_doc).await {
        Ok(_) => info!("Pull replication configured"),
        Err(crate::couchdb::CouchError::Conflict(_)) => {
            info!("Pull replication already configured");
        }
        Err(e) => {
            warn!(error = %e, "Failed to configure pull replication");
            return Err(e.into());
        }
    }

    Ok(())
}

fn parse_scheme(url: &str) -> &str {
    if url.starts_with("https") {
        "https"
    } else {
        "http"
    }
}

fn parse_host(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .trim_end_matches('/')
}
