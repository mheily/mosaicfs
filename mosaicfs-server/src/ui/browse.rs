//! End-user file browser (§3.1–§3.8).
//!
//! Four handlers: page, list, navigate, open.
//! Replaces the old admin-oriented `views::browse_page`.

use std::cmp::Ordering;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Form,
};
use chrono::Local;
use serde::Deserialize;
use tera::Context;
use tower_sessions::Session;

use crate::handlers::vfs::dir_id_for;
use crate::state::AppState;
use crate::ui::{page_ctx, render};
use crate::readdir::{self, ReaddirEntry};
use crate::ui::open::open_file_by_id;

const PAGE_SIZE: usize = 50;

// ── GET /ui/browse ────────────────────────────────────────────────────────

pub async fn page(session: Session) -> Response {
    let mut ctx = page_ctx(&session).await;
    ctx.insert("title", "Browse — MosaicFS");
    ctx.insert("initial_path", "/");
    render("browse_app.html", &ctx)
}

// ── GET /ui/browse/list ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,
    pub q: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub offset: Option<usize>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Response {
    let path = query.path.as_deref().unwrap_or("/").to_string();
    let q = query.q.as_deref().unwrap_or("").to_string();
    let sort = query.sort.as_deref().unwrap_or("name").to_string();
    let order = query.order.as_deref().unwrap_or("asc").to_string();
    let offset = query.offset.unwrap_or(0);

    let subdirs = fetch_subdirs(&state, &path).await;
    let files = fetch_files(&state, &path).await;

    let mut rows: Vec<serde_json::Value> = Vec::new();

    for d in &subdirs {
        let name = d.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if !q.is_empty() && !name.to_lowercase().contains(&q.to_lowercase()) {
            continue;
        }
        let vpath = d
            .get("virtual_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        rows.push(serde_json::json!({
            "type": "dir",
            "name": name,
            "virtual_path": vpath,
            "size": 0u64,
            "size_display": "—",
            "mtime": "",
            "date_display": "",
        }));
    }

    for f in &files {
        let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if !q.is_empty() && !name.to_lowercase().contains(&q.to_lowercase()) {
            continue;
        }
        let size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let mtime_str = f.get("mtime").and_then(|v| v.as_str()).unwrap_or("");
        let date_display = if let Ok(dt) = mtime_str.parse::<chrono::DateTime<chrono::Utc>>() {
            dt.with_timezone(&Local).format("%Y-%m-%d").to_string()
        } else {
            "".to_string()
        };
        rows.push(serde_json::json!({
            "type": "file",
            "name": name,
            "file_id": f.get("file_id").and_then(|v| v.as_str()).unwrap_or(""),
            "virtual_path": format!("{}/{}", path.trim_end_matches('/'), name),
            "size": size,
            "size_display": fmt_size(size),
            "mtime": mtime_str,
            "date_display": date_display,
        }));
    }

    sort_rows(&mut rows, &sort, &order);

    let dir_count = subdirs.len();
    let total_after_filter = rows.len();
    let has_more = total_after_filter > offset + PAGE_SIZE;
    let page_rows: Vec<_> = rows
        .into_iter()
        .skip(offset)
        .take(PAGE_SIZE)
        .collect();

    let next_offset = offset + PAGE_SIZE;

    let mut ctx = Context::new();
    ctx.insert("path", &path);
    ctx.insert("rows", &page_rows);
    ctx.insert("dir_count", &dir_count);
    ctx.insert("offset", &offset);
    ctx.insert("next_offset", &next_offset);
    ctx.insert("has_more", &has_more);
    ctx.insert("sort", &sort);
    ctx.insert("order", &order);
    ctx.insert("q", &q);
    ctx.insert("total_after_filter", &total_after_filter);
    render("browse_list.html", &ctx)
}

// ── GET /ui/browse/navigate ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NavigateQuery {
    pub path: Option<String>,
}

pub async fn navigate(
    State(state): State<Arc<AppState>>,
    _session: Session,
    Query(query): Query<NavigateQuery>,
) -> Response {
    let path = query.path.as_deref().unwrap_or("/").to_string();

    let subdirs = fetch_subdirs(&state, &path).await;
    let files = fetch_files(&state, &path).await;

    let mut rows: Vec<serde_json::Value> = Vec::new();

    for d in &subdirs {
        let name = d.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let vpath = d
            .get("virtual_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        rows.push(serde_json::json!({
            "type": "dir",
            "name": name,
            "virtual_path": vpath,
            "size": 0u64,
            "size_display": "—",
            "mtime": "",
            "date_display": "",
        }));
    }

    for f in &files {
        let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let mtime_str = f.get("mtime").and_then(|v| v.as_str()).unwrap_or("");
        let date_display = if let Ok(dt) = mtime_str.parse::<chrono::DateTime<chrono::Utc>>() {
            dt.with_timezone(&Local).format("%Y-%m-%d").to_string()
        } else {
            "".to_string()
        };
        rows.push(serde_json::json!({
            "type": "file",
            "name": name,
            "file_id": f.get("file_id").and_then(|v| v.as_str()).unwrap_or(""),
            "virtual_path": format!("{}/{}", path.trim_end_matches('/'), name),
            "size": size,
            "size_display": fmt_size(size),
            "mtime": mtime_str,
            "date_display": date_display,
        }));
    }

    sort_rows(&mut rows, "name", "asc");

    let dir_count = subdirs.len();

    let mut ctx = Context::new();
    ctx.insert("path", &path);
    ctx.insert("rows", &rows);
    ctx.insert("dir_count", &dir_count);
    ctx.insert("offset", &0usize);
    ctx.insert("next_offset", &PAGE_SIZE);
    ctx.insert("has_more", &false);
    ctx.insert("sort", "name");
    ctx.insert("order", "asc");
    ctx.insert("q", "");
    ctx.insert("total_after_filter", &rows.len());
    render("browse_list.html", &ctx)
}

// ── POST /ui/browse/open ──────────────────────────────────────────────────

pub async fn open(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<OpenForm>,
) -> Response {
    let virtual_path = form.path.clone();

    let entry = match lookup_entry_by_virtual_path(&state, &virtual_path).await {
        Some(e) => e,
        None => {
            return flash_response(&session, "File not found.");
        }
    };

    let file_id = &entry.file_id;
    match open_file_by_id(&state, file_id).await {
        Ok(local_path) => flash_response(&session, &format!("Opened {}", local_path)),
        Err(e) => flash_response(&session, &e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct OpenForm {
    pub path: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────

async fn fetch_subdirs(state: &AppState, path: &str) -> Vec<serde_json::Value> {
    match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| {
                d.get("type").and_then(|v| v.as_str()) == Some("virtual_directory")
                    && d.get("parent_path").and_then(|v| v.as_str()) == Some(path)
            })
            .map(|d| {
                serde_json::json!({
                    "virtual_path": d.get("virtual_path").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": d.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect(),
        Err(_) => vec![],
    }
}

async fn fetch_files(state: &AppState, path: &str) -> Vec<serde_json::Value> {
    let dir_doc = match state.db.get_document(&dir_id_for(path)).await {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let subdirs: Vec<String> = match state.db.all_docs_by_prefix("dir::", true).await {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| row.doc)
            .filter(|d| {
                d.get("type").and_then(|v| v.as_str()) == Some("virtual_directory")
                    && d.get("parent_path").and_then(|v| v.as_str()) == Some(path)
            })
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(str::to_string))
            .collect(),
        Err(_) => vec![],
    };

    use crate::handlers::vfs::parse_step_based_mounts_pub;

    let mounts_json = dir_doc
        .get("mounts")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let mount_entries = parse_step_based_mounts_pub(&mounts_json);

    let mut inherited_steps = readdir::collect_inherited_steps(&state.db, path)
        .await
        .unwrap_or_default();
    if let Some(dir_steps) = dir_doc.get("steps").and_then(|v| v.as_array()) {
        for s in dir_steps {
            if let Ok(step) = serde_json::from_value::<mosaicfs_common::documents::Step>(s.clone())
            {
                inherited_steps.push(step);
            }
        }
    }

    match readdir::evaluate_readdir(
        &state.db,
        &state.label_cache,
        &state.access_cache,
        &mount_entries,
        &inherited_steps,
        &subdirs,
    )
    .await
    {
        Ok(entries) => entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "file_id": &e.file_id,
                    "name": &e.name,
                    "size": e.size,
                    "mtime": e.mtime.to_rfc3339(),
                    "source_node_id": &e.source_node_id,
                    "source_export_path": &e.source_export_path,
                })
            })
            .collect(),
        Err(_) => vec![],
    }
}

fn sort_rows(rows: &mut [serde_json::Value], sort: &str, order: &str) {
    let desc = order == "desc";
    rows.sort_by(|a, b| {
        let a_is_dir = a.get("type").and_then(|v| v.as_str()) == Some("dir");
        let b_is_dir = b.get("type").and_then(|v| v.as_str()) == Some("dir");
        match (a_is_dir, b_is_dir) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }

        let ord = match sort {
            "size" => {
                let sa = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                let sb = b.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                sa.cmp(&sb)
            }
            "mtime" => {
                let ma = a.get("mtime").and_then(|v| v.as_str()).unwrap_or("");
                let mb = b.get("mtime").and_then(|v| v.as_str()).unwrap_or("");
                ma.cmp(mb)
            }
            _ => {
                let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                na.to_lowercase().cmp(&nb.to_lowercase())
            }
        };

        if desc { ord.reverse() } else { ord }
    });
}

async fn lookup_entry_by_virtual_path(
    state: &AppState,
    virtual_path: &str,
) -> Option<ReaddirEntry> {
    let path = virtual_path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return None;
    }

    let filename = path.rsplit('/').next()?;
    let parent = if path == format!("/{}", filename) {
        "/"
    } else {
        &path[..path.len() - filename.len() - 1]
    };
    if parent.is_empty() {
        return None;
    }

    let files = fetch_files(state, parent).await;
    let file_id = files
        .iter()
        .find(|f| f.get("name").and_then(|v| v.as_str()) == Some(filename))?
        .get("file_id")?
        .as_str()?;

    let file_doc = state.db.get_document(file_id).await.ok()?;
    let source_node_id = file_doc
        .get("source")
        .and_then(|s| s.get("node_id"))
        .and_then(|v| v.as_str())?
        .to_string();
    let export_path = file_doc
        .get("source")
        .and_then(|s| s.get("export_path"))
        .and_then(|v| v.as_str())?
        .to_string();
    let size = file_doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    let mtime_str = file_doc
        .get("mtime")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mtime = mtime_str
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|_| chrono::Utc::now());
    let mime_type = file_doc
        .get("mime_type")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let inode = file_doc.get("inode").and_then(|v| v.as_u64()).unwrap_or(0);

    Some(ReaddirEntry {
        name: filename.to_string(),
        file_id: file_id.to_string(),
        inode,
        size,
        mtime,
        mime_type,
        source_node_id,
        source_export_path: export_path,
        mount_id: String::new(),
    })
}

fn flash_response(_session: &Session, msg: &str) -> Response {
    let flash_html = format!(
        r#"<div class="flash" style="background:#fef2f2;border-left:4px solid #dc2626;color:#991b1b;padding:0.75rem 1rem;border-radius:4px">{}</div>"#,
        html_escape(msg)
    );
    (StatusCode::OK, Html(flash_html)).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Size formatting filter (§3.5).
/// 0 → "0", 1..1023 → "1K", ≥1024 → ceil(promote until ≤999).
pub fn fmt_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0".to_string();
    }
    if bytes < 1024 {
        return "1K".to_string();
    }
    let units = ["K", "M", "G"];
    let mut val = bytes as f64;
    let mut unit_idx = 0;
    loop {
        val /= 1024.0;
        if unit_idx + 1 >= units.len() {
            let rounded = val.ceil() as u64;
            return format!("{}{}", rounded, units[unit_idx]);
        }
        if val.ceil() <= 999.0 {
            let rounded = val.ceil() as u64;
            return format!("{}{}", rounded, units[unit_idx]);
        }
        unit_idx += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_size_zero() {
        assert_eq!(fmt_size(0), "0");
    }

    #[test]
    fn test_fmt_size_1k() {
        assert_eq!(fmt_size(1), "1K");
        assert_eq!(fmt_size(885), "1K");
        assert_eq!(fmt_size(1023), "1K");
    }

    #[test]
    fn test_fmt_size_1m() {
        assert_eq!(fmt_size(1024 * 1024), "1M");
    }

    #[test]
    fn test_fmt_size_2m() {
        assert_eq!(fmt_size(1524 * 1024), "2M");
    }

    #[test]
    fn test_fmt_size_999m() {
        assert_eq!(fmt_size(999 * 1024 * 1024), "999M");
    }

    #[test]
    fn test_fmt_size_1g() {
        assert_eq!(fmt_size(1000 * 1024 * 1024), "1G");
    }
}
