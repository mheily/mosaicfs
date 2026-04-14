//! Shared replication helpers used by both the agent job runner and the
//! server REST handlers.
//!
//! Only the logic that both sides must agree on lives here: how a storage
//! backend document maps to a replication target id, whether a backend doc
//! counts as an active target, and how a rule's `default_result` string maps
//! onto the typed `StepResult` enum. The full agent subsystem and the full
//! server handler each keep their own code path; this module is the contract
//! surface between them.

use serde_json::Value;

use crate::documents::StepResult;

/// Build the CouchDB document id for a storage backend by its user-facing name.
pub fn storage_backend_doc_id(name: &str) -> String {
    format!("storage_backend::{}", name)
}

/// Whether a storage-backend document represents an active replication target.
///
/// An active target has `type == "storage_backend"`, `enabled == true`, and
/// `mode == "target"`. The agent uses this to decide which backends to drive
/// jobs against; the server uses it when filtering lists for the UI.
pub fn is_active_storage_target(doc: &Value) -> bool {
    doc.get("type").and_then(|v| v.as_str()) == Some("storage_backend")
        && doc.get("enabled").and_then(|v| v.as_bool()) == Some(true)
        && doc.get("mode").and_then(|v| v.as_str()) == Some("target")
}

/// Parse a rule's `default_result` string. Unknown values default to
/// `Exclude` — the conservative choice, matching the server's validation.
pub fn parse_default_result(s: &str) -> StepResult {
    match s {
        "include" => StepResult::Include,
        _ => StepResult::Exclude,
    }
}
