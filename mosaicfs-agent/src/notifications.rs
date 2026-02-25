//! Notification helpers for the agent.
//!
//! Provides `emit_notification()` and `resolve_notification()` that write
//! `notification::` documents to CouchDB with proper upsert semantics
//! (preserving `first_seen`, incrementing `occurrence_count`).

use chrono::Utc;
use tracing::warn;

use crate::couchdb::CouchClient;

/// Emit (or update) a notification document.
///
/// The document ID is deterministic: `notification::{node_id}::{condition_key}`.
/// On first write a new document is created.  On subsequent writes the existing
/// document is updated: `last_seen` is refreshed, `occurrence_count` is
/// incremented, and `first_seen` is preserved from the original.
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

    // Try to fetch existing document for upsert
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

/// Resolve an existing notification by setting `status: "resolved"` and
/// `resolved_at` to the current timestamp.  No-op if the document does not
/// exist or is already resolved.
pub async fn resolve_notification(
    db: &CouchClient,
    node_id: &str,
    condition_key: &str,
) {
    let doc_id = format!("notification::{}::{}", node_id, condition_key);

    let mut doc = match db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => return, // Nothing to resolve
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
