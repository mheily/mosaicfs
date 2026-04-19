//! Read-only admin views (Phase 2 + 3).

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    response::Response,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tera::Context;
use tower_sessions::Session;

use crate::admin::{page_ctx, render};
use crate::credentials;
use crate::handlers::vfs::dir_id_for;
use crate::state::AppState;

fn fmt_duration(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn now_str() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

// ── status ──

pub async fn status_page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Status — MosaicFS");
    render("status.html", &ctx)
}

pub async fn status_panel(State(state): State<Arc<AppState>>) -> Response {
    let mut ctx = Context::new();
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("uptime", &fmt_duration(state.started_at.elapsed().as_secs()));

    let (couch_status, couch_class) = match state.db.db_info().await {
        Ok(_) => ("reachable", "ok"),
        Err(_) => ("unreachable", "err"),
    };
    ctx.insert("couch_status", couch_status);
    ctx.insert("couch_class", couch_class);

    let needs_bootstrap = state.data_dir.join("bootstrap_token").exists();
    ctx.insert("needs_bootstrap", if needs_bootstrap { "yes" } else { "no" });

    let (node_count, heartbeat_recent) = match state.db.all_docs_by_prefix("node::", true).await {
        Ok(resp) => {
            let mut total = 0u64;
            let mut recent = 0u64;
            let now = Utc::now();
            for row in resp.rows {
                if let Some(doc) = row.doc {
                    if doc.get("type").and_then(|v| v.as_str()) == Some("node") {
                        total += 1;
                        if let Some(hb) = doc.get("last_heartbeat").and_then(|v| v.as_str()) {
                            if let Ok(ts) = hb.parse::<DateTime<Utc>>() {
                                if (now - ts).num_seconds() <= 60 {
                                    recent += 1;
                                }
                            }
                        }
                    }
                }
            }
            (total, recent)
        }
        Err(_) => (0, 0),
    };
    ctx.insert("node_count", &node_count);
    ctx.insert("heartbeat_recent", &heartbeat_recent);

    let notification_count = match state
        .db
        .all_docs_by_prefix("notification::", true)
        .await
    {
        Ok(resp) => resp
            .rows
            .iter()
            .filter_map(|r| r.doc.as_ref())
            .filter(|d| {
                d.get("type").and_then(|v| v.as_str()) == Some("notification")
                    && d.get("status").and_then(|v| v.as_str()).unwrap_or("active") == "active"
            })
            .count() as u64,
        Err(_) => 0,
    };
    ctx.insert("notification_count", &notification_count);

    ctx.insert("now", &now_str());
    render("status_panel.html", &ctx)
}

// ── nodes ──

pub async fn nodes_page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Nodes — MosaicFS");
    render("nodes.html", &ctx)
}

pub async fn nodes_panel(State(state): State<Arc<AppState>>) -> Response {
    let resp = state.db.all_docs_by_prefix("node::", true).await;
    let now = Utc::now();
    let nodes: Vec<serde_json::Value> = match resp {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("node"))
            .map(|d| {
                let id = d
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_start_matches("node::")
                    .to_string();
                let hostname = d
                    .get("friendly_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let hb_raw = d
                    .get("last_heartbeat")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let (age, status, status_class) = if let Ok(ts) = hb_raw.parse::<DateTime<Utc>>() {
                    let secs = (now - ts).num_seconds().max(0) as u64;
                    let status_text = if secs <= 60 {
                        "online"
                    } else if secs <= 600 {
                        "stale"
                    } else {
                        "offline"
                    };
                    let class = match status_text {
                        "online" => "ok",
                        "stale" => "warn",
                        _ => "err",
                    };
                    (fmt_duration(secs), status_text.to_string(), class.to_string())
                } else {
                    (
                        "unknown".to_string(),
                        "unknown".to_string(),
                        "muted".to_string(),
                    )
                };
                serde_json::json!({
                    "node_id": id,
                    "hostname": hostname,
                    "last_heartbeat": hb_raw,
                    "age": age,
                    "status": status,
                    "status_class": status_class,
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    let mut ctx = Context::new();
    ctx.insert("nodes", &nodes);
    ctx.insert("now", &now_str());
    render("nodes_panel.html", &ctx)
}

// ── notifications ──

pub async fn notifications_page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Notifications — MosaicFS");
    render("notifications.html", &ctx)
}

pub async fn notifications_panel(State(state): State<Arc<AppState>>) -> Response {
    let resp = state.db.all_docs_by_prefix("notification::", true).await;
    let items: Vec<serde_json::Value> = match resp {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| {
                d.get("type").and_then(|v| v.as_str()) == Some("notification")
                    && d.get("status").and_then(|v| v.as_str()).unwrap_or("active") == "active"
            })
            .map(|d| {
                let severity = d
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                let severity_class = match severity.as_str() {
                    "critical" | "error" => "err",
                    "warning" => "warn",
                    _ => "muted",
                };
                let full_id = d
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_start_matches("notification::")
                    .to_string();
                serde_json::json!({
                    "id": full_id,
                    "severity": severity,
                    "severity_class": severity_class,
                    "category": d.get("category").and_then(|v| v.as_str()).unwrap_or(""),
                    "title": d.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    "message": d.get("message").and_then(|v| v.as_str()).unwrap_or(""),
                    "created_at": d.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    let mut ctx = Context::new();
    ctx.insert("items", &items);
    ctx.insert("now", &now_str());
    render("notifications_panel.html", &ctx)
}

// ── replication ──

pub async fn replication_page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Replication — MosaicFS");
    render("replication.html", &ctx)
}

pub async fn replication_panel(State(state): State<Arc<AppState>>) -> Response {
    let backends: Vec<serde_json::Value> = match state
        .db
        .all_docs_by_prefix("storage_backend::", true)
        .await
    {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("storage_backend"))
            .map(|d| {
                serde_json::json!({
                    "name": d.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "kind": d.get("backend").and_then(|v| v.as_str())
                        .or_else(|| d.get("backend_type").and_then(|v| v.as_str()))
                        .or_else(|| d.get("kind").and_then(|v| v.as_str()))
                        .unwrap_or(""),
                    "enabled": d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect(),
        Err(_) => vec![],
    };

    let rules: Vec<serde_json::Value> = match state
        .db
        .all_docs_by_prefix("replication_rule::", true)
        .await
    {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("replication_rule"))
            .map(|d| {
                let id = d
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_start_matches("replication_rule::")
                    .to_string();
                let source = d
                    .get("source")
                    .and_then(|v| v.get("node_id"))
                    .and_then(|v| v.as_str())
                    .or_else(|| d.get("source_path").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                serde_json::json!({
                    "id": id,
                    "source": source,
                    "target": d.get("target_name").and_then(|v| v.as_str())
                        .or_else(|| d.get("target_backend").and_then(|v| v.as_str()))
                        .or_else(|| d.get("target").and_then(|v| v.as_str()))
                        .unwrap_or(""),
                    "enabled": d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect(),
        Err(_) => vec![],
    };

    let mut ctx = Context::new();
    ctx.insert("backends", &backends);
    ctx.insert("rules", &rules);
    ctx.insert("now", &now_str());
    render("replication_panel.html", &ctx)
}

// ── Node detail ──

pub async fn node_detail_page(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(node_id): Path<String>,
) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", &format!("Node {node_id} — MosaicFS"));
    ctx.insert("node_id", &node_id);

    match state.db.get_document(&format!("node::{node_id}")).await {
        Ok(doc) => {
            let friendly_name = doc
                .get("friendly_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let hb = doc
                .get("last_heartbeat")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mounts: Vec<serde_json::Value> = doc
                .get("network_mounts")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            ctx.insert("friendly_name", &friendly_name);
            ctx.insert("last_heartbeat", &hb);
            ctx.insert("mounts", &mounts);
            ctx.insert("exists", &true);
        }
        Err(_) => {
            ctx.insert("exists", &false);
        }
    }
    render("node_detail.html", &ctx)
}

// ── Storage backends ──

pub async fn storage_backends_page(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Storage backends — MosaicFS");

    let backends: Vec<serde_json::Value> = match state
        .db
        .all_docs_by_prefix("storage_backend::", true)
        .await
    {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("storage_backend"))
            .map(|d| {
                serde_json::json!({
                    "name": d.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "backend": d.get("backend").and_then(|v| v.as_str()).unwrap_or(""),
                    "enabled": d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                    "created_at": d.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    ctx.insert("backends", &backends);
    render("storage_backends.html", &ctx)
}

// ── Settings: credentials ──

pub async fn settings_credentials_page(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Credentials — MosaicFS");

    // One-shot: newly created (ak, sk)
    if let Ok(Some((ak, sk))) = session
        .remove::<(String, String)>(crate::admin::NEW_SECRET_KEY)
        .await
    {
        ctx.insert("created_access_key", &ak);
        ctx.insert("created_secret_key", &sk);
    }

    let creds: Vec<serde_json::Value> = match credentials::list_credentials(&state.db).await {
        Ok(list) => list
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "access_key_id": c.get("access_key_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "enabled": c.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                    "created_at": c.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
                    "last_used": c.get("last_used").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    ctx.insert("credentials", &creds);
    render("settings_credentials.html", &ctx)
}

// ── Settings: backup ──

pub async fn settings_backup_page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Backup — MosaicFS");
    render("settings_backup.html", &ctx)
}

// ── VFS ──

/// Loads node list and per-node watched paths from CouchDB.
/// Paths come from `node.storage[].watch_paths_on_fs`, which are the actual
/// directories the agent indexes — the right thing to offer as mount source paths.
/// Returns (nodes_json, node_exports_json) as pre-serialized strings for embedding in templates.
async fn load_node_data(state: &AppState) -> (String, String) {
    let mut nodes: Vec<serde_json::Value> = Vec::new();
    let mut node_exports: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    if let Ok(r) = state.db.all_docs_by_prefix("node::", true).await {
        for doc in r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("node"))
        {
            let node_id = doc
                .get("_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim_start_matches("node::")
                .to_string();
            let friendly_name = doc
                .get("friendly_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| node_id.clone());

            // Collect every watch_path across all storage entries for this node.
            let mut paths: Vec<String> = doc
                .get("storage")
                .and_then(|v| v.as_array())
                .map(|entries| {
                    entries
                        .iter()
                        .flat_map(|e| {
                            e.get("watch_paths_on_fs")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default()
                        })
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();

            paths.sort();
            paths.dedup();

            nodes.push(serde_json::json!({"node_id": node_id, "friendly_name": friendly_name}));
            node_exports.insert(node_id, paths);
        }
    }

    (
        serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&node_exports).unwrap_or_else(|_| "{}".to_string()),
    )
}

pub async fn vfs_page(State(state): State<Arc<AppState>>, session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Virtual Filesystem — MosaicFS");

    let dirs: Vec<serde_json::Value> = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => {
            let mut dirs: Vec<_> = r
                .rows
                .into_iter()
                .filter_map(|row| row.doc)
                .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("virtual_directory"))
                .map(|d| {
                    let mount_count = d
                        .get("mounts")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    serde_json::json!({
                        "virtual_path": d.get("virtual_path").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": d.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "mount_count": mount_count,
                        "is_system": d.get("system").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                })
                .collect();
            dirs.sort_by(|a, b| {
                let pa = a.get("virtual_path").and_then(|v| v.as_str()).unwrap_or("");
                let pb = b.get("virtual_path").and_then(|v| v.as_str()).unwrap_or("");
                pa.cmp(pb)
            });
            dirs
        }
        Err(_) => vec![],
    };
    ctx.insert("dirs", &dirs);
    render("vfs.html", &ctx)
}

pub async fn vfs_new_page(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<VfsNewQuery>,
) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Create Directory — MosaicFS VFS");
    let parent = query.parent.as_deref().unwrap_or("").to_string();
    ctx.insert("parent_path", &parent);
    let (nodes_json, node_exports_json) = load_node_data(&state).await;
    ctx.insert("nodes_json", &nodes_json);
    ctx.insert("node_exports_json", &node_exports_json);
    render("vfs_new.html", &ctx)
}

#[derive(Deserialize)]
pub struct VfsNewQuery {
    pub parent: Option<String>,
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub path: Option<String>,
}

pub async fn browse_page(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<BrowseQuery>,
) -> Response {
    let path = query.path.as_deref().unwrap_or("/").to_string();
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Browse — MosaicFS");
    ctx.insert("current_path", &path);
    ctx.insert("crumbs", &build_breadcrumbs(&path));

    let doc_id = dir_id_for(&path);
    match state.db.get_document(&doc_id).await {
        Ok(doc) => {
            let mount_count = doc
                .get("mounts")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            ctx.insert("dir_exists", &true);
            ctx.insert("is_system", &doc.get("system").and_then(|v| v.as_bool()).unwrap_or(false));
            ctx.insert("mount_count", &mount_count);
        }
        Err(_) => {
            ctx.insert("dir_exists", &false);
            ctx.insert("is_system", &false);
            ctx.insert("mount_count", &0usize);
        }
    }

    let all_dirs = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r,
        Err(_) => {
            ctx.insert("subdirs", &Vec::<serde_json::Value>::new());
            ctx.insert("files", &Vec::<serde_json::Value>::new());
            return render("browse.html", &ctx);
        }
    };

    let mut subdirs: Vec<serde_json::Value> = all_dirs
        .rows
        .iter()
        .filter_map(|row| row.doc.as_ref())
        .filter(|d| {
            d.get("type").and_then(|v| v.as_str()) == Some("virtual_directory")
                && d.get("parent_path").and_then(|v| v.as_str()) == Some(path.as_str())
        })
        .map(|d| {
            serde_json::json!({
                "virtual_path": d.get("virtual_path").and_then(|v| v.as_str()).unwrap_or(""),
                "name": d.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "mount_count": d.get("mounts").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
            })
        })
        .collect();
    subdirs.sort_by(|a, b| {
        a.get("name").and_then(|v| v.as_str()).unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });
    ctx.insert("subdirs", &subdirs);

    // Evaluate the mount pipeline to get the file list for this directory,
    // the same way the FUSE readdir does.
    let child_dir_names: Vec<String> = subdirs
        .iter()
        .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect();

    let files: Vec<serde_json::Value> = if let Ok(dir_doc) =
        state.db.get_document(&dir_id_for(&path)).await
    {
        use crate::handlers::vfs::parse_step_based_mounts_pub;
        use crate::readdir as rd;

        let mounts_json = dir_doc
            .get("mounts")
            .cloned()
            .unwrap_or(serde_json::json!([]));
        let mount_entries = parse_step_based_mounts_pub(&mounts_json);

        let mut inherited_steps =
            rd::collect_inherited_steps(&state.db, &path)
                .await
                .unwrap_or_default();
        if let Some(dir_steps) = dir_doc.get("steps").and_then(|v| v.as_array()) {
            for s in dir_steps {
                if let Ok(step) =
                    serde_json::from_value::<mosaicfs_common::documents::Step>(s.clone())
                {
                    inherited_steps.push(step);
                }
            }
        }

        match rd::evaluate_readdir(
            &state.db,
            &state.label_cache,
            &state.access_cache,
            &mount_entries,
            &inherited_steps,
            &child_dir_names,
        )
        .await
        {
            Ok(entries) => entries
                .iter()
                .map(|e| {
                    let uuid = e.file_id.strip_prefix("file::").unwrap_or(&e.file_id);
                    let mut labels: Vec<String> =
                        state.label_cache.get_labels(uuid).into_iter().collect();
                    labels.sort();
                    serde_json::json!({
                        "file_id": e.file_id,
                        "name": e.name,
                        "source_node": e.source_node_id,
                        "mime_type": e.mime_type.as_deref().unwrap_or(""),
                        "labels": labels,
                    })
                })
                .collect(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };
    ctx.insert("files", &files);

    render("browse.html", &ctx)
}

fn build_breadcrumbs(path: &str) -> Vec<serde_json::Value> {
    let mut crumbs =
        vec![serde_json::json!({"label": "root", "path": "/", "is_current": path == "/"})];
    if path == "/" {
        return crumbs;
    }
    let parts: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let mut accumulated = String::new();
    for (i, part) in parts.iter().enumerate() {
        accumulated = format!("{}/{}", accumulated, part);
        crumbs.push(serde_json::json!({
            "label": part,
            "path": accumulated.clone(),
            "is_current": i == parts.len() - 1,
        }));
    }
    crumbs
}

#[derive(Deserialize)]
pub struct VfsDirQuery {
    pub path: Option<String>,
    pub source_type: Option<String>,
}

pub async fn vfs_dir_page(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<VfsDirQuery>,
) -> Response {
    let path = query.path.as_deref().unwrap_or("/").to_string();
    let default_source_type = query.source_type.as_deref().unwrap_or("node").to_string();
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", &format!("{} — MosaicFS VFS", path));
    let (nodes_json, node_exports_json) = load_node_data(&state).await;
    ctx.insert("nodes_json", &nodes_json);
    ctx.insert("node_exports_json", &node_exports_json);

    let doc_id = dir_id_for(&path);
    match state.db.get_document(&doc_id).await {
        Ok(doc) => {
            let dir = serde_json::json!({
                "virtual_path": doc.get("virtual_path").and_then(|v| v.as_str()).unwrap_or(&path),
                "name": doc.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "enforce_steps_on_children": doc.get("enforce_steps_on_children").and_then(|v| v.as_bool()).unwrap_or(false),
                "is_system": doc.get("system").and_then(|v| v.as_bool()).unwrap_or(false),
            });
            let mounts: Vec<serde_json::Value> = doc
                .get("mounts")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(build_mount_ctx)
                .collect();
            ctx.insert("dir", &dir);
            ctx.insert("mounts", &mounts);
            ctx.insert("exists", &true);
        }
        Err(_) => {
            ctx.insert("exists", &false);
        }
    }
    render("vfs_dir.html", &ctx)
}

fn build_mount_ctx(m: &serde_json::Value) -> serde_json::Value {
    let mount_id = m
        .get("mount_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (source_type, source_display) = if let Some(src) = m.get("source") {
        if let Some(nid) = src.get("node_id").and_then(|v| v.as_str()) {
            let ep = src.get("export_path").and_then(|v| v.as_str()).unwrap_or("/");
            ("node", format!("{} @ {}", nid, ep))
        } else if let Some(lbl) = src.get("label").and_then(|v| v.as_str()) {
            ("label", lbl.to_string())
        } else if let Some(fid) = src.get("federated_import_id").and_then(|v| v.as_str()) {
            ("federated", fid.to_string())
        } else {
            ("unknown", "unknown source".to_string())
        }
    } else if let Some(nid) = m.get("node_id").and_then(|v| v.as_str()) {
        let ep = m.get("export_path").and_then(|v| v.as_str()).unwrap_or("/");
        ("node", format!("{} @ {}", nid, ep))
    } else {
        ("unknown", "unknown source".to_string())
    };

    let steps: Vec<serde_json::Value> = m
        .get("steps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| {
            serde_json::json!({
                "op": s.get("op").and_then(|v| v.as_str()).unwrap_or(""),
                "invert": s.get("invert").and_then(|v| v.as_bool()).unwrap_or(false),
                "on_match": s.get("on_match").and_then(|v| v.as_str()).unwrap_or(""),
                "params_summary": step_params_summary(s),
            })
        })
        .collect();

    serde_json::json!({
        "mount_id": mount_id,
        "source_type": source_type,
        "source_display": source_display,
        "strategy": m.get("strategy").and_then(|v| v.as_str()).unwrap_or("flatten"),
        "source_prefix": m.get("source_prefix").and_then(|v| v.as_str()).unwrap_or(""),
        "conflict_policy": m.get("conflict_policy").and_then(|v| v.as_str()).unwrap_or("last_write_wins"),
        "default_result": m.get("default_result").and_then(|v| v.as_str()).unwrap_or("include"),
        "steps": steps,
    })
}

fn step_params_summary(s: &serde_json::Value) -> String {
    match s.get("op").and_then(|v| v.as_str()).unwrap_or("") {
        "glob" | "regex" | "mime" => s
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|p| format!("pattern={}", p))
            .unwrap_or_default(),
        "age" => {
            let days = s.get("days").and_then(|v| v.as_i64()).unwrap_or(0);
            let cmp = s.get("comparison").and_then(|v| v.as_str()).unwrap_or("lt");
            format!("days {} {}", cmp, days)
        }
        "size" => {
            let bytes = s.get("bytes").and_then(|v| v.as_i64()).unwrap_or(0);
            let cmp = s.get("comparison").and_then(|v| v.as_str()).unwrap_or("lt");
            format!("bytes {} {}", cmp, bytes)
        }
        "node" => s
            .get("node_id")
            .and_then(|v| v.as_str())
            .map(|n| format!("node_id={}", n))
            .unwrap_or_default(),
        "label" => s
            .get("label")
            .and_then(|v| v.as_str())
            .map(|l| format!("label={}", l))
            .unwrap_or_default(),
        "access_age" => {
            let days = s.get("days").and_then(|v| v.as_i64()).unwrap_or(0);
            let cmp = s.get("comparison").and_then(|v| v.as_str()).unwrap_or("lt");
            let missing = s.get("missing").and_then(|v| v.as_str()).unwrap_or("include");
            format!("days {} {} missing={}", cmp, days, missing)
        }
        "replicated" => {
            let target = s.get("target").and_then(|v| v.as_str()).unwrap_or("*");
            let status = s.get("status").and_then(|v| v.as_str()).unwrap_or("*");
            format!("target={} status={}", target, status)
        }
        "annotation" => s
            .get("plugin_name")
            .and_then(|v| v.as_str())
            .map(|p| format!("plugin={}", p))
            .unwrap_or_default(),
        _ => String::new(),
    }
}
