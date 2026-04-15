//! Read-only admin views (Phase 2).

use std::sync::Arc;

use axum::{extract::State, response::Response};
use chrono::{DateTime, Utc};
use tera::Context;
use tower_sessions::Session;

use crate::admin::{base_ctx, render, user_for_ctx};
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
    let user = user_for_ctx(&session).await;
    let mut ctx = base_ctx(user.as_deref());
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
                    && d.get("acknowledged").and_then(|v| v.as_bool()) != Some(true)
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
    let user = user_for_ctx(&session).await;
    let mut ctx = base_ctx(user.as_deref());
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
    let user = user_for_ctx(&session).await;
    let mut ctx = base_ctx(user.as_deref());
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
                    && d.get("acknowledged").and_then(|v| v.as_bool()) != Some(true)
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
                serde_json::json!({
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
    let user = user_for_ctx(&session).await;
    let mut ctx = base_ctx(user.as_deref());
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
                    "kind": d.get("backend_type").and_then(|v| v.as_str())
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
                serde_json::json!({
                    "id": id,
                    "source": d.get("source_path").and_then(|v| v.as_str())
                        .or_else(|| d.get("source").and_then(|v| v.as_str()))
                        .unwrap_or(""),
                    "target": d.get("target_backend").and_then(|v| v.as_str())
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
