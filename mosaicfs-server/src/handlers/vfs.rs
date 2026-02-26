use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use mosaicfs_common::documents::{ConflictPolicy, MountEntry, MountSource, MountStrategy, Step, StepResult};

use crate::couchdb::CouchError;
use crate::readdir;
use crate::state::AppState;

fn dir_id(virtual_path: &str) -> String {
    dir_id_for(virtual_path)
}

/// Public version of dir_id for use by other modules.
pub fn dir_id_for(virtual_path: &str) -> String {
    if virtual_path == "/" {
        "dir::root".to_string()
    } else {
        let hash = hex::encode(Sha256::digest(virtual_path.as_bytes()));
        format!("dir::{}", hash)
    }
}

fn error_json(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": code, "message": message } })
}

fn strip_internals(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_rev");
        if let Some(id) = obj.remove("_id") {
            obj.insert("id".to_string(), id);
        }
    }
}

fn validate_virtual_path(path: &str) -> Result<(), &'static str> {
    if !path.starts_with('/') {
        return Err("Virtual path must start with /");
    }
    if path.starts_with("/federation/") {
        return Err("Cannot use /federation/ prefix");
    }
    if path.contains("//") {
        return Err("Virtual path must not contain //");
    }
    Ok(())
}

// ── VFS listing ──

#[derive(Deserialize, Default)]
pub struct VfsQuery {
    pub path: Option<String>,
}

pub async fn list_vfs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VfsQuery>,
) -> impl IntoResponse {
    let path = query.path.as_deref().unwrap_or("/");
    let doc_id = dir_id(path);

    // Get the directory document
    let dir_doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Directory not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    // Get child directories
    let all_dirs = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let subdirs: Vec<serde_json::Value> = all_dirs
        .rows
        .into_iter()
        .filter_map(|row| {
            let doc = row.doc?;
            let parent = doc.get("parent_path")?.as_str()?;
            if parent == path {
                Some(serde_json::json!({
                    "name": doc.get("name"),
                    "virtual_path": doc.get("virtual_path"),
                    "type": "directory",
                }))
            } else {
                None
            }
        })
        .collect();

    let mounts = dir_doc.get("mounts").cloned().unwrap_or(serde_json::json!([]));

    // Parse mounts for evaluation (supports both structured and step-based formats)
    let mount_entries = parse_step_based_mounts(&mounts);

    // Check readdir cache
    let dir_rev = dir_doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("");
    let child_dir_names: Vec<String> = subdirs
        .iter()
        .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let file_entries = if let Some(cached) = state.readdir_cache.get(path, dir_rev) {
        cached
    } else {
        // Collect inherited steps from ancestors
        let inherited_steps = match readdir::collect_inherited_steps(&state.db, path).await {
            Ok(s) => s,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
            }
        };

        match readdir::evaluate_readdir(
            &state.db,
            &state.label_cache,
            &state.access_cache,
            &mount_entries,
            &inherited_steps,
            &child_dir_names,
        )
        .await
        {
            Ok(entries) => {
                state.readdir_cache.put(path, dir_rev, entries.clone());
                entries
            }
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
            }
        }
    };

    let files = readdir::entries_to_json(&file_entries);

    (StatusCode::OK, Json(serde_json::json!({
        "path": path,
        "directories": subdirs,
        "mounts": mounts,
        "files": files,
    })))
}

#[derive(Deserialize, Default)]
pub struct TreeQuery {
    pub path: Option<String>,
    pub depth: Option<u32>,
}

pub async fn get_tree(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TreeQuery>,
) -> impl IntoResponse {
    let path = query.path.as_deref().unwrap_or("/");
    let max_depth = query.depth.unwrap_or(3).min(10);

    let all_dirs = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let dirs: Vec<serde_json::Value> = all_dirs
        .rows
        .into_iter()
        .filter_map(|row| {
            let doc = row.doc?;
            let vpath = doc.get("virtual_path")?.as_str()?;

            // Include if it's under the requested path (or is the path itself)
            if vpath == path || vpath.starts_with(&format!("{}/", path.trim_end_matches('/'))) {
                // Check depth
                let rel = if path == "/" {
                    vpath.to_string()
                } else {
                    vpath.strip_prefix(path)?.to_string()
                };
                let depth = rel.matches('/').count() as u32;
                if depth <= max_depth {
                    Some(serde_json::json!({
                        "name": doc.get("name"),
                        "virtual_path": vpath,
                        "has_children": false,
                    }))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({
        "path": path,
        "tree": dirs,
    })))
}

/// Convert a JSON mounts array (possibly in step-based format) into `MountEntry` objects.
fn parse_step_based_mounts(mounts_json: &serde_json::Value) -> Vec<MountEntry> {
    let arr = match mounts_json.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .enumerate()
        .filter_map(|(i, m)| {
            // Try direct deserialization first (structured format)
            if let Ok(entry) = serde_json::from_value::<MountEntry>(m.clone()) {
                return Some(entry);
            }
            // Fall back to step-based format: {"steps":[{"type":"node","node_id":"..."},{"type":"path_prefix","prefix":"..."}]}
            let steps = m.get("steps")?.as_array()?;
            let mut node_id: Option<String> = None;
            let mut export_path = "/".to_string();
            let mut label: Option<String> = None;
            for step in steps {
                match step.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "node" => {
                        node_id = step.get("node_id").and_then(|v| v.as_str()).map(String::from);
                    }
                    "path_prefix" => {
                        if let Some(p) = step.get("prefix").and_then(|v| v.as_str()) {
                            export_path = p.to_string();
                        }
                    }
                    "label" => {
                        label = step.get("label").and_then(|v| v.as_str()).map(String::from);
                    }
                    _ => {}
                }
            }
            let source = if let Some(nid) = node_id {
                MountSource::Node { node_id: nid, export_path }
            } else if let Some(lbl) = label {
                MountSource::Label { label: lbl }
            } else {
                return None;
            };
            Some(MountEntry {
                mount_id: format!("m{}", i),
                source,
                strategy: MountStrategy::Flatten,
                source_prefix: None,
                steps: vec![],
                default_result: StepResult::Include,
                conflict_policy: ConflictPolicy::LastWriteWins,
            })
        })
        .collect()
}

// ── Directory CRUD ──

#[derive(Deserialize)]
pub struct CreateDirectoryRequest {
    #[serde(rename = "path")]
    pub virtual_path: String,
    pub name: String,
}

pub async fn create_directory(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateDirectoryRequest>,
) -> impl IntoResponse {
    if let Err(msg) = validate_virtual_path(&body.virtual_path) {
        return (StatusCode::BAD_REQUEST, Json(error_json("validation", msg)));
    }

    let doc_id = dir_id(&body.virtual_path);

    // Check if already exists
    if state.db.get_document(&doc_id).await.is_ok() {
        return (StatusCode::CONFLICT, Json(error_json("conflict", "Directory already exists")));
    }

    // Compute parent path
    let parent_path = if body.virtual_path == "/" {
        None
    } else {
        let parts: Vec<&str> = body.virtual_path.trim_end_matches('/').rsplitn(2, '/').collect();
        if parts.len() > 1 && !parts[1].is_empty() {
            Some(parts[1].to_string())
        } else {
            Some("/".to_string())
        }
    };

    // Generate random inode (>= 1000)
    let inode: u64 = loop {
        let v: u64 = rand::random();
        if v >= 1000 {
            break v;
        }
    };

    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "virtual_directory",
        "inode": inode,
        "virtual_path": body.virtual_path,
        "name": body.name,
        "parent_path": parent_path,
        "created_at": Utc::now().to_rfc3339(),
        "enforce_steps_on_children": false,
        "mounts": [],
    });

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => {
            let mut result = doc.clone();
            strip_internals(&mut result);
            (StatusCode::CREATED, Json(result))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

pub async fn get_directory(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let virtual_path = format!("/{}", path);
    let doc_id = dir_id(&virtual_path);

    match state.db.get_document(&doc_id).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            // Also try root
            if virtual_path == "/" || path.is_empty() {
                match state.db.get_document("dir::root").await {
                    Ok(mut doc) => {
                        strip_internals(&mut doc);
                        return (StatusCode::OK, Json(doc));
                    }
                    Err(_) => {}
                }
            }
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Directory not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

pub async fn patch_directory(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let virtual_path = format!("/{}", path);
    let doc_id = dir_id(&virtual_path);

    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Directory not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    // Apply updates
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        doc["name"] = serde_json::Value::String(name.to_string());
    }
    if let Some(enforce) = body.get("enforce_steps_on_children").and_then(|v| v.as_bool()) {
        doc["enforce_steps_on_children"] = serde_json::Value::Bool(enforce);
    }
    if let Some(mounts) = body.get("mounts") {
        doc["mounts"] = mounts.clone();
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

#[derive(Deserialize, Default)]
pub struct DeleteDirQuery {
    pub force: Option<bool>,
}

pub async fn delete_directory(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Query(query): Query<DeleteDirQuery>,
) -> impl IntoResponse {
    let virtual_path = format!("/{}", path);
    let doc_id = dir_id(&virtual_path);

    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Directory not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    // System directories cannot be deleted
    if doc.get("system").and_then(|v| v.as_bool()) == Some(true) {
        return (StatusCode::FORBIDDEN, Json(error_json("forbidden", "Cannot delete system directory")));
    }

    // Check for children
    let all_dirs = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let children: Vec<serde_json::Value> = all_dirs
        .rows
        .iter()
        .filter_map(|row| {
            let d = row.doc.as_ref()?;
            let parent = d.get("parent_path")?.as_str()?;
            if parent == virtual_path {
                Some(d.clone())
            } else {
                None
            }
        })
        .collect();

    if !children.is_empty() && query.force != Some(true) {
        return (StatusCode::CONFLICT, Json(error_json("conflict", "Directory has children. Use ?force=true to cascade delete.")));
    }

    // Delete children if force
    if query.force == Some(true) {
        for child in &children {
            if let (Some(id), Some(rev)) = (
                child.get("_id").and_then(|v| v.as_str()),
                child.get("_rev").and_then(|v| v.as_str()),
            ) {
                let _ = state.db.delete_document(id, rev).await;
            }
        }
    }

    // Delete the directory
    let rev = doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("");
    match state.db.delete_document(&doc_id, rev).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

// ── Directory Preview ──

#[derive(Deserialize)]
pub struct PreviewRequest {
    pub mounts: Vec<MountEntry>,
    #[serde(default)]
    pub inherited_steps: Vec<Step>,
}

pub async fn preview_directory(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(body): Json<PreviewRequest>,
) -> impl IntoResponse {
    let virtual_path = format!("/{}", path);

    // Get child directories for this path
    let all_dirs = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let child_dir_names: Vec<String> = all_dirs
        .rows
        .iter()
        .filter_map(|row| {
            let doc = row.doc.as_ref()?;
            let parent = doc.get("parent_path")?.as_str()?;
            if parent == virtual_path {
                doc.get("name").and_then(|v| v.as_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();

    match readdir::evaluate_readdir(
        &state.db,
        &state.label_cache,
        &state.access_cache,
        &body.mounts,
        &body.inherited_steps,
        &child_dir_names,
    )
    .await
    {
        Ok(entries) => {
            let files = readdir::entries_to_json(&entries);
            (StatusCode::OK, Json(serde_json::json!({
                "path": virtual_path,
                "files": files,
                "total": files.len(),
            })))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

/// Ensure root directory exists on startup
pub async fn ensure_root_directory(state: &AppState) -> anyhow::Result<()> {
    match state.db.get_document("dir::root").await {
        Ok(_) => Ok(()),
        Err(CouchError::NotFound(_)) => {
            let doc = serde_json::json!({
                "_id": "dir::root",
                "type": "virtual_directory",
                "inode": 1,
                "virtual_path": "/",
                "name": "",
                "parent_path": null,
                "system": true,
                "created_at": Utc::now().to_rfc3339(),
                "enforce_steps_on_children": false,
                "mounts": [],
            });
            state.db.put_document("dir::root", &doc).await?;
            tracing::info!("Created root directory document");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_id() {
        assert_eq!(dir_id("/"), "dir::root");
        let id = dir_id("/documents/work");
        assert!(id.starts_with("dir::"));
        assert_ne!(id, "dir::root");
        // Deterministic
        assert_eq!(dir_id("/documents/work"), id);
    }

    #[test]
    fn test_validate_virtual_path() {
        assert!(validate_virtual_path("/").is_ok());
        assert!(validate_virtual_path("/documents").is_ok());
        assert!(validate_virtual_path("/documents/work").is_ok());
        assert!(validate_virtual_path("relative").is_err());
        assert!(validate_virtual_path("/federation/peer").is_err());
        assert!(validate_virtual_path("/bad//path").is_err());
    }
}
