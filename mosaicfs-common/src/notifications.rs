//! Shared notification publisher.
//!
//! Writes `notification::{node_id}::{condition_key}` documents to CouchDB
//! with upsert semantics: `first_seen` is preserved, `last_seen` is
//! refreshed, and `occurrence_count` is incremented on each repeated emit.
//!
//! The HTTP handler that serves notifications to the UI lives in the server
//! crate — only the publisher side is shared.

use chrono::Utc;
use tracing::warn;

use crate::couchdb::CouchClient;

/// Emit or update a notification. Document id is deterministic:
/// `notification::{node_id}::{condition_key}`. Use `node_id = "control_plane"`
/// for server-originated notifications.
pub async fn emit_notification(
    db: &CouchClient,
    node_id: &str,
    component: &str,
    condition_key: &str,
    severity: &str,
    title: &str,
    message: &str,
    actions: Option<Vec<serde_json::Value>>,
) {
    let doc_id = format!("notification::{}::{}", node_id, condition_key);
    let now = Utc::now().to_rfc3339();

    let existing = db.get_document(&doc_id).await.ok();

    let (first_seen, occurrence_count, rev) = match &existing {
        Some(doc) => {
            let fs = doc
                .get("first_seen")
                .and_then(|v| v.as_str())
                .unwrap_or(&now)
                .to_string();
            let count = doc
                .get("occurrence_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                + 1;
            let rev = doc
                .get("_rev")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (fs, count, rev)
        }
        None => (now.clone(), 1, None),
    };

    let mut doc = serde_json::json!({
        "_id": doc_id,
        "type": "notification",
        "source": {
            "node_id": node_id,
            "component": component,
        },
        "condition_key": condition_key,
        "severity": severity,
        "status": "active",
        "title": title,
        "message": message,
        "first_seen": first_seen,
        "last_seen": now,
        "occurrence_count": occurrence_count,
    });

    if let Some(acts) = actions {
        doc["actions"] = serde_json::Value::Array(acts);
    }

    if let Some(rev) = rev {
        doc["_rev"] = serde_json::Value::String(rev);
    }

    if let Err(e) = db.put_document(&doc_id, &doc).await {
        warn!(doc_id = %doc_id, error = %e, "Failed to emit notification");
    }
}

/// Mark an existing notification as resolved. No-op if missing or already resolved.
pub async fn resolve_notification(db: &CouchClient, node_id: &str, condition_key: &str) {
    let doc_id = format!("notification::{}::{}", node_id, condition_key);

    let mut doc = match db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => return,
    };

    let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "resolved" {
        return;
    }

    doc["status"] = serde_json::Value::String("resolved".to_string());
    doc["resolved_at"] = serde_json::Value::String(Utc::now().to_rfc3339());

    if let Err(e) = db.put_document(&doc_id, &doc).await {
        warn!(doc_id = %doc_id, error = %e, "Failed to resolve notification");
    }
}

/// Convenience wrapper for server-originated notifications
/// (node_id = "control_plane", no actions).
pub async fn emit_control_plane_notification(
    db: &CouchClient,
    component: &str,
    condition_key: &str,
    severity: &str,
    title: &str,
    message: &str,
) {
    emit_notification(
        db,
        "control_plane",
        component,
        condition_key,
        severity,
        title,
        message,
        None,
    )
    .await;
}

/// Convenience wrapper to resolve a control-plane notification.
pub async fn resolve_control_plane_notification(db: &CouchClient, condition_key: &str) {
    resolve_notification(db, "control_plane", condition_key).await;
}
