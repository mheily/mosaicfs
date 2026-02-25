//! Replication subsystem — core agent component.
//!
//! Runs in-process with the agent alongside the crawler and VFS layer.
//! Subscribes to file events, evaluates replication rules using the step
//! pipeline engine, and coordinates uploads to configured storage backends.
//!
//! State is persisted in a local SQLite database (`replication.db`).
//! CouchDB `replica` documents and annotation documents are written for
//! cross-system visibility and Tier 4b failover.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use bytes::Bytes;
use chrono::{DateTime, NaiveTime, Timelike, Utc};
use mosaicfs_common::backend::remote_key as compute_remote_key;
use mosaicfs_common::documents::StepResult;
use mosaicfs_common::steps::{StepContext, evaluate_steps};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::backend;
use crate::couchdb::CouchClient;

// ── Event types ──────────────────────────────────────────────────────────────

/// File-level events emitted by the crawler and watcher.
#[derive(Debug, Clone)]
pub enum FileEvent {
    Added { file_id: String, file_doc: serde_json::Value },
    Modified { file_id: String, file_doc: serde_json::Value },
    Deleted { file_id: String },
    AccessUpdated { file_id: String },
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// A parsed replication target with its config.
#[derive(Clone, Debug)]
struct ReplicationTarget {
    name: String,
    backend_type: String,
    prefix: String,
    schedule: Option<(NaiveTime, NaiveTime)>,
    bandwidth_limit_mbps: Option<i32>,
    workers: usize,
    retention_days: i32,
    remove_unmatched: bool,
    backend_doc: serde_json::Value,
    credential_doc: Option<serde_json::Value>,
}

/// A parsed replication rule.
#[derive(Clone, Debug)]
struct ReplicationRule {
    rule_id: String,
    name: String,
    target_name: String,
    source_node_id: String,
    source_path_prefix: Option<String>,
    steps: Vec<mosaicfs_common::documents::Step>,
    default_result: StepResult,
}

// ── SQLite State ──────────────────────────────────────────────────────────────

fn open_db(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path).context("Failed to open replication.db")?;
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;

        CREATE TABLE IF NOT EXISTS replication_state (
            file_id         TEXT NOT NULL,
            target_name     TEXT NOT NULL,
            replicated_at   TEXT NOT NULL,
            source_mtime    TEXT NOT NULL,
            source_size     INTEGER NOT NULL,
            remote_key      TEXT NOT NULL,
            checksum        TEXT,
            PRIMARY KEY (file_id, target_name)
        );

        CREATE TABLE IF NOT EXISTS deletion_log (
            file_id         TEXT NOT NULL,
            target_name     TEXT NOT NULL,
            deleted_at      TEXT NOT NULL,
            retain_until    TEXT,
            remote_key      TEXT NOT NULL,
            purged          INTEGER DEFAULT 0,
            PRIMARY KEY (file_id, target_name)
        );

        CREATE INDEX IF NOT EXISTS idx_deletion_retain ON deletion_log (purged, retain_until);

        CREATE TABLE IF NOT EXISTS upload_queue (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id         TEXT NOT NULL,
            target_name     TEXT NOT NULL,
            queued_at       TEXT NOT NULL,
            priority        INTEGER DEFAULT 0,
            UNIQUE(file_id, target_name)
        );

        CREATE TABLE IF NOT EXISTS rebuild_state (
            key             TEXT PRIMARY KEY,
            value           TEXT NOT NULL
        );
    ").context("Failed to create replication tables")?;
    Ok(conn)
}

// ── Step context for rule evaluation ─────────────────────────────────────────

struct ReplicationStepContext {
    labels: std::collections::HashSet<String>,
    last_access: Option<DateTime<Utc>>,
    replicas: Vec<(String, String)>, // (target_name, status)
}

impl StepContext for ReplicationStepContext {
    fn has_label(&self, _file_uuid: &str, label: &str) -> bool {
        self.labels.contains(label)
    }

    fn last_access(&self, _file_id: &str) -> Option<DateTime<Utc>> {
        self.last_access
    }

    fn has_replica(&self, _file_uuid: &str, target: Option<&str>, status: Option<&str>) -> bool {
        self.replicas.iter().any(|(t, s)| {
            target.map_or(true, |filt| t == filt) && status.map_or(true, |filt| s == filt)
        })
    }

    fn has_annotation(&self, _file_uuid: &str, _plugin_name: &str) -> bool {
        false
    }
}

// ── Token bucket rate limiter ─────────────────────────────────────────────────

struct TokenBucket {
    capacity_bytes: f64,
    available: f64,
    last_refill: Instant,
    bytes_per_sec: f64,
}

impl TokenBucket {
    fn new(limit_mbps: i32) -> Self {
        let bytes_per_sec = (limit_mbps as f64) * 1_000_000.0 / 8.0;
        Self {
            capacity_bytes: bytes_per_sec * 2.0, // 2s burst
            available: bytes_per_sec * 2.0,
            last_refill: Instant::now(),
            bytes_per_sec,
        }
    }

    /// Wait until `bytes` tokens are available, then consume them.
    async fn consume(&mut self, bytes: usize) {
        let bytes = bytes as f64;
        loop {
            let elapsed = self.last_refill.elapsed().as_secs_f64();
            self.available = (self.available + elapsed * self.bytes_per_sec)
                .min(self.capacity_bytes);
            self.last_refill = Instant::now();

            if self.available >= bytes {
                self.available -= bytes;
                return;
            }

            // Wait for enough tokens to accumulate
            let deficit = bytes - self.available;
            let wait_secs = deficit / self.bytes_per_sec;
            tokio::time::sleep(Duration::from_secs_f64(wait_secs.min(1.0))).await;
        }
    }
}

// ── Schedule window ───────────────────────────────────────────────────────────

/// Parse a schedule string "HH:MM-HH:MM" into (start, end) NaiveTime.
fn parse_schedule(s: &str) -> Option<(NaiveTime, NaiveTime)> {
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return None;
    }
    let parse_time = |t: &str| {
        let hm: Vec<&str> = t.trim().splitn(2, ':').collect();
        if hm.len() != 2 { return None; }
        let h: u32 = hm[0].parse().ok()?;
        let m: u32 = hm[1].parse().ok()?;
        NaiveTime::from_hms_opt(h, m, 0)
    };
    let start = parse_time(parts[0])?;
    let end = parse_time(parts[1])?;
    Some((start, end))
}

/// Return true if the current local time is within the schedule window.
fn in_schedule_window(window: Option<&(NaiveTime, NaiveTime)>) -> bool {
    let Some((start, end)) = window else { return true; }; // No window = always active
    let now = chrono::Local::now().time().with_second(0).unwrap();
    if start <= end {
        now >= *start && now < *end
    } else {
        // Wraps midnight
        now >= *start || now < *end
    }
}

// ── Main subsystem ────────────────────────────────────────────────────────────

/// Handle to the replication subsystem for sending events.
pub struct ReplicationHandle {
    pub tx: mpsc::UnboundedSender<FileEvent>,
}

impl ReplicationHandle {
    pub fn send(&self, event: FileEvent) {
        let _ = self.tx.send(event);
    }
}

/// Configuration for the replication subsystem.
pub struct ReplicationConfig {
    pub node_id: String,
    pub state_dir: PathBuf,
    pub db: CouchClient,
    /// How often to flush annotation batches (seconds).
    pub flush_interval_s: u64,
    /// How often to run the periodic full scan (seconds). Default 86400 (daily).
    pub full_scan_interval_s: u64,
}

/// Start the replication subsystem. Returns a handle for sending events.
pub fn start(config: ReplicationConfig) -> anyhow::Result<ReplicationHandle> {
    let (tx, rx) = mpsc::unbounded_channel();

    let db_path = config.state_dir.join("replication.db");
    let db_exists = db_path.exists();

    let conn = open_db(&db_path)?;

    let needs_rebuild = if !db_exists {
        warn!("replication.db not found — entering rebuild mode");
        true
    } else {
        // Check rebuild_state flag
        let flag: Option<String> = conn
            .query_row(
                "SELECT value FROM rebuild_state WHERE key = 'needs_rebuild'",
                [],
                |row| row.get(0),
            )
            .ok();
        flag.as_deref() == Some("1")
    };

    if needs_rebuild {
        // Emit a manifest_rebuild_needed notification to CouchDB
        let db_clone = config.db.clone();
        let node_id = config.node_id.clone();
        tokio::spawn(async move {
            crate::notifications::emit_notification(
                &db_clone,
                &node_id,
                "replication",
                "manifest_rebuild_needed",
                "warning",
                "Replication state rebuild needed",
                "The local replication manifest was lost. Rebuilding from targets on next full scan.",
                None,
            ).await;
        });
    }

    // Spawn the main event loop
    let node_id = config.node_id.clone();
    let db = config.db.clone();
    let flush_interval_s = config.flush_interval_s;
    let full_scan_interval_s = config.full_scan_interval_s;
    let state_dir = config.state_dir.clone();

    tokio::spawn(async move {
        if let Err(e) = run_event_loop(
            rx, node_id, db, state_dir, flush_interval_s, full_scan_interval_s, needs_rebuild,
        ).await {
            error!(error = %e, "Replication subsystem crashed");
        }
    });

    Ok(ReplicationHandle { tx })
}

async fn run_event_loop(
    mut rx: mpsc::UnboundedReceiver<FileEvent>,
    node_id: String,
    db: CouchClient,
    state_dir: PathBuf,
    flush_interval_s: u64,
    full_scan_interval_s: u64,
    needs_rebuild: bool,
) -> anyhow::Result<()> {
    let db_path = state_dir.join("replication.db");
    let conn = Arc::new(std::sync::Mutex::new(open_db(&db_path)?));

    // In-memory annotation accumulator: file_id -> { target_name -> status }
    let mut pending_annotations: HashMap<String, HashMap<String, String>> = HashMap::new();

    let mut flush_ticker = time::interval(Duration::from_secs(flush_interval_s));
    let mut scan_ticker = time::interval(Duration::from_secs(full_scan_interval_s));
    let mut deletion_sweep_ticker = time::interval(Duration::from_secs(3600)); // hourly
    let mut upload_queue_ticker = time::interval(Duration::from_secs(10));

    // Pending upload queue: (file_id, target_name) -> file_doc (in memory for quick delivery)
    let mut pending_uploads: HashMap<(String, String), serde_json::Value> = HashMap::new();

    info!("Replication subsystem started (node_id={})", node_id);

    if needs_rebuild {
        // Schedule an immediate full scan to rebuild
        scan_ticker.reset_immediately();
    }

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    None => break, // Channel closed
                    Some(ev) => {
                        if let Err(e) = handle_event(
                            ev, &node_id, &db, &conn,
                            &mut pending_annotations, &mut pending_uploads,
                        ).await {
                            warn!(error = %e, "Error processing replication event");
                        }
                    }
                }
            }

            _ = flush_ticker.tick() => {
                flush_annotations(&db, &node_id, &mut pending_annotations).await;
            }

            _ = scan_ticker.tick() => {
                if let Err(e) = run_full_scan(&db, &conn, &node_id, &mut pending_annotations, &mut pending_uploads).await {
                    warn!(error = %e, "Full scan failed");
                }
            }

            _ = deletion_sweep_ticker.tick() => {
                if let Err(e) = sweep_deletion_log(&db, &conn).await {
                    warn!(error = %e, "Deletion sweep failed");
                }
            }

            _ = upload_queue_ticker.tick() => {
                process_upload_queue(&db, &conn, &node_id, &mut pending_annotations).await;
            }
        }
    }

    // Final flush
    flush_annotations(&db, &node_id, &mut pending_annotations).await;
    Ok(())
}

async fn handle_event(
    event: FileEvent,
    node_id: &str,
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
    pending_uploads: &mut HashMap<(String, String), serde_json::Value>,
) -> anyhow::Result<()> {
    match event {
        FileEvent::Added { file_id, file_doc } | FileEvent::Modified { file_id, file_doc } => {
            process_file_event(&file_id, &file_doc, node_id, db, conn, pending_annotations, pending_uploads).await
        }
        FileEvent::Deleted { file_id } => {
            process_deletion_event(&file_id, node_id, db, conn, pending_annotations).await
        }
        FileEvent::AccessUpdated { file_id } => {
            // Re-evaluate rules for this file to detect un-replication
            if let Ok(file_doc) = db.get_document(&file_id).await {
                process_file_event(&file_id, &file_doc, node_id, db, conn, pending_annotations, pending_uploads).await
            } else {
                Ok(())
            }
        }
    }
}

async fn process_file_event(
    file_id: &str,
    file_doc: &serde_json::Value,
    node_id: &str,
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
    pending_uploads: &mut HashMap<(String, String), serde_json::Value>,
) -> anyhow::Result<()> {
    // Only process files from this node
    let doc_node_id = file_doc
        .get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str())
        .unwrap_or("");
    if doc_node_id != node_id {
        return Ok(());
    }

    let (rules, targets) = load_rules_and_targets(db).await?;
    let file_uuid = file_id.strip_prefix("file::").unwrap_or(file_id);

    // Build step context
    let ctx = build_step_context(db, file_id, file_uuid, conn).await;

    for rule in &rules {
        // Check source filter
        if rule.source_node_id != "*" && rule.source_node_id != node_id {
            continue;
        }
        if let Some(ref prefix) = rule.source_path_prefix {
            let export_path = file_doc
                .get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str())
                .unwrap_or("");
            if !export_path.starts_with(prefix.as_str()) {
                continue;
            }
        }

        // Evaluate steps
        let result = evaluate_steps(
            &rule.steps,
            &parse_file_doc(file_doc),
            file_id,
            &rule.default_result,
            &ctx,
        );

        if result != StepResult::Include {
            // File doesn't match this rule — check if it was previously replicated
            check_un_replication(file_id, &rule.target_name, &targets, conn, pending_annotations).await;
            continue;
        }

        let target = match targets.get(&rule.target_name) {
            Some(t) => t,
            None => {
                warn!(target = %rule.target_name, "Replication target not found");
                continue;
            }
        };

        // Check if already current
        let existing = get_replication_state(conn, file_id, &rule.target_name);
        let source_mtime = file_doc.get("mtime").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let source_size = file_doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        if let Some(ref state) = existing {
            if state.source_mtime == source_mtime && state.source_size == source_size {
                // Already current
                continue;
            }
        }

        // Check schedule window
        if !in_schedule_window(target.schedule.as_ref()) {
            // Queue for later
            queue_upload(conn, file_id, &rule.target_name);
            update_annotation_status(pending_annotations, file_id, &rule.target_name, "pending");
            continue;
        }

        // Queue for upload
        pending_uploads.insert((file_id.to_string(), rule.target_name.clone()), file_doc.clone());
        update_annotation_status(pending_annotations, file_id, &rule.target_name, "stale");
    }

    Ok(())
}

async fn process_deletion_event(
    file_id: &str,
    node_id: &str,
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
) -> anyhow::Result<()> {
    let (_, targets) = load_rules_and_targets(db).await?;

    // Find all replicated targets for this file
    let replicated_targets: Vec<(String, String)> = {
        let guard = conn.lock().unwrap();
        let mut stmt = guard.prepare(
            "SELECT target_name, remote_key FROM replication_state WHERE file_id = ?"
        ).unwrap();
        stmt.query_map([file_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .flatten()
            .collect()
    };

    for (target_name, remote_key) in replicated_targets {
        let target = match targets.get(&target_name) {
            Some(t) => t,
            None => continue,
        };

        let deleted_at = Utc::now().to_rfc3339();
        let retain_until = if target.retention_days > 0 {
            Some((Utc::now() + chrono::Duration::days(target.retention_days as i64)).to_rfc3339())
        } else {
            None
        };

        {
            let guard = conn.lock().unwrap();
            // Move to deletion_log
            guard.execute(
                "INSERT OR REPLACE INTO deletion_log (file_id, target_name, deleted_at, retain_until, remote_key, purged)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![file_id, target_name, deleted_at, retain_until, remote_key],
            ).unwrap_or_default();

            // Remove from replication_state
            guard.execute(
                "DELETE FROM replication_state WHERE file_id = ?1 AND target_name = ?2",
                params![file_id, target_name],
            ).unwrap_or_default();
        }

        // If no retention, delete immediately
        if target.retention_days == 0 {
            let backend_result = build_backend(target);
            if let Ok(adapter) = backend_result {
                let _ = adapter.delete(&remote_key).await;
                let guard = conn.lock().unwrap();
                guard.execute(
                    "UPDATE deletion_log SET purged = 1 WHERE file_id = ?1 AND target_name = ?2",
                    params![file_id, target_name],
                ).unwrap_or_default();
            }
        }

        // Remove annotation entry for this target
        if let Some(ann) = pending_annotations.get_mut(file_id) {
            ann.remove(&target_name);
        }
    }

    Ok(())
}

async fn check_un_replication(
    file_id: &str,
    target_name: &str,
    targets: &HashMap<String, ReplicationTarget>,
    conn: &Arc<std::sync::Mutex<Connection>>,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
) {
    // Check if this file was replicated to this target
    let has_replica = {
        let guard = conn.lock().unwrap();
        guard.query_row(
            "SELECT COUNT(*) FROM replication_state WHERE file_id = ?1 AND target_name = ?2",
            params![file_id, target_name],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0
    };

    if !has_replica { return; }

    let target = match targets.get(target_name) { Some(t) => t, None => return };

    if target.remove_unmatched {
        // Move to deletion log with retention
        let remote_key: Option<String> = {
            let guard = conn.lock().unwrap();
            guard.query_row(
                "SELECT remote_key FROM replication_state WHERE file_id = ?1 AND target_name = ?2",
                params![file_id, target_name],
                |row| row.get(0),
            ).ok()
        };

        if let Some(rk) = remote_key {
            let retain_until = if target.retention_days > 0 {
                Some((Utc::now() + chrono::Duration::days(target.retention_days as i64)).to_rfc3339())
            } else {
                None
            };
            let guard = conn.lock().unwrap();
            guard.execute(
                "INSERT OR REPLACE INTO deletion_log (file_id, target_name, deleted_at, retain_until, remote_key, purged)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![file_id, target_name, Utc::now().to_rfc3339(), retain_until, rk],
            ).unwrap_or_default();
            guard.execute(
                "DELETE FROM replication_state WHERE file_id = ?1 AND target_name = ?2",
                params![file_id, target_name],
            ).unwrap_or_default();
        }
    } else {
        // Mark as frozen
        update_annotation_status(pending_annotations, file_id, target_name, "frozen");
    }
}

async fn process_upload_queue(
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
    node_id: &str,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
) {
    let (_, targets) = match load_rules_and_targets(db).await {
        Ok(r) => r,
        Err(_) => return,
    };

    // Get queued items that are within the schedule window for their target
    let queued: Vec<(String, String)> = {
        let guard = conn.lock().unwrap();
        let mut stmt = match guard.prepare(
            "SELECT file_id, target_name FROM upload_queue ORDER BY priority DESC, id ASC LIMIT 100"
        ) {
            Ok(s) => s,
            Err(_) => return,
        };
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(|rows| rows.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
    };

    for (file_id, target_name) in queued {
        let target = match targets.get(&target_name) {
            Some(t) => t,
            None => continue,
        };

        // Only process if within schedule window
        if !in_schedule_window(target.schedule.as_ref()) {
            continue;
        }

        // Fetch file document
        let file_doc = match db.get_document(&file_id).await {
            Ok(d) => d,
            Err(_) => {
                // Remove from queue — file may be deleted
                let guard = conn.lock().unwrap();
                guard.execute(
                    "DELETE FROM upload_queue WHERE file_id = ?1 AND target_name = ?2",
                    params![file_id, target_name],
                ).unwrap_or_default();
                continue;
            }
        };

        // Perform upload
        let node_id_placeholder = String::new();
        let upload_result = perform_upload(&file_id, &file_doc, target, db).await;

        match upload_result {
            Ok((remote_key, checksum)) => {
                let source_mtime = file_doc.get("mtime").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let source_size = file_doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

                {
                    let guard = conn.lock().unwrap();
                    guard.execute(
                        "INSERT OR REPLACE INTO replication_state
                         (file_id, target_name, replicated_at, source_mtime, source_size, remote_key, checksum)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            file_id, target_name, Utc::now().to_rfc3339(),
                            source_mtime, source_size as i64, remote_key, checksum
                        ],
                    ).unwrap_or_default();

                    guard.execute(
                        "DELETE FROM upload_queue WHERE file_id = ?1 AND target_name = ?2",
                        params![file_id, target_name],
                    ).unwrap_or_default();
                }

                update_annotation_status(pending_annotations, &file_id, &target_name, "current");

                // Write replica document to CouchDB
                let file_uuid = file_id.strip_prefix("file::").unwrap_or(&file_id);
                let _ = write_replica_doc(db, &file_id, &target_name, &remote_key, &file_doc, target).await;
            }
            Err(e) => {
                warn!(file_id = %file_id, target = %target_name, error = %e, "Upload failed");
                update_annotation_status(pending_annotations, &file_id, &target_name, "failed");

                let err_str = e.to_string();
                if err_str.contains("connect") || err_str.contains("dns") || err_str.contains("timeout") {
                    crate::notifications::emit_notification(
                        db, node_id, "replication",
                        &format!("replication_target_unreachable:{}", target_name),
                        "error", "Replication target unreachable",
                        &format!("Cannot reach target '{}': {}", target_name, err_str),
                        None,
                    ).await;
                } else {
                    crate::notifications::emit_notification(
                        db, node_id, "replication", "replication_error",
                        "error", "Replication upload failed",
                        &format!("Upload to '{}' failed for {}: {}", target_name, file_id, err_str),
                        None,
                    ).await;
                }
            }
        }
    }

    // Check for backlog
    let queue_size: i64 = {
        let guard = conn.lock().unwrap();
        guard.query_row("SELECT COUNT(*) FROM upload_queue", [], |row| row.get(0))
            .unwrap_or(0)
    };
    if queue_size > 1000 {
        crate::notifications::emit_notification(
            db, node_id, "replication", "replication_backlog",
            "warning", "Replication backlog",
            &format!("{} files waiting in upload queue.", queue_size),
            None,
        ).await;
    } else {
        crate::notifications::resolve_notification(db, node_id, "replication_backlog").await;
    }
}

/// Actually upload a file to a backend target. Returns (remote_key, checksum).
async fn perform_upload(
    file_id: &str,
    file_doc: &serde_json::Value,
    target: &ReplicationTarget,
    db: &CouchClient,
) -> anyhow::Result<(String, Option<String>)> {
    let file_uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
    let filename = file_doc.get("name").and_then(|v| v.as_str()).unwrap_or("file");
    let prefix = &target.prefix;

    let rkey = compute_remote_key(prefix, file_uuid, filename);

    // Fetch file content from the agent's local transfer endpoint
    // (In practice, this reads from the local filesystem via the VFS tiered access)
    let export_path = file_doc
        .get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str())
        .unwrap_or("");

    if export_path.is_empty() {
        bail!("File has no export_path");
    }

    let data = tokio::fs::read(export_path)
        .await
        .with_context(|| format!("Failed to read file at {}", export_path))?;

    // Compute checksum
    use sha2::{Digest, Sha256};
    let checksum = hex::encode(Sha256::digest(&data));

    let adapter = build_backend(target)?;

    // Rate limit if configured
    let bytes_to_send = data.len();
    // (Rate limiting is applied per-target in a real implementation via shared TokenBucket)

    adapter.upload(&rkey, Bytes::from(data)).await
        .with_context(|| format!("Upload to {} target '{}' failed", target.backend_type, target.name))?;

    info!(
        file_id = %file_id,
        target = %target.name,
        key = %rkey,
        size = bytes_to_send,
        "File replicated"
    );

    Ok((rkey, Some(checksum)))
}

fn build_backend(target: &ReplicationTarget) -> anyhow::Result<Box<dyn mosaicfs_common::backend::BackendAdapter>> {
    backend::from_backend_doc(&target.backend_doc, target.credential_doc.as_ref())
}

async fn write_replica_doc(
    db: &CouchClient,
    file_id: &str,
    target_name: &str,
    remote_key: &str,
    file_doc: &serde_json::Value,
    target: &ReplicationTarget,
) -> anyhow::Result<()> {
    let file_uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
    let doc_id = format!("replica::{}::{}", file_uuid, target_name);

    let source_node_id = file_doc
        .get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str())
        .unwrap_or("");

    // Get existing rev if any
    let rev = db.get_document(&doc_id).await.ok()
        .and_then(|d| d.get("_rev").and_then(|v| v.as_str()).map(|s| s.to_string()));

    let mut doc = serde_json::json!({
        "_id": doc_id,
        "type": "replica",
        "file_id": file_id,
        "target_name": target_name,
        "backend": target.backend_type,
        "remote_key": remote_key,
        "replicated_at": Utc::now().to_rfc3339(),
        "source_mtime": file_doc.get("mtime").unwrap_or(&serde_json::Value::Null),
        "source_size": file_doc.get("size").unwrap_or(&serde_json::Value::Null),
        "status": "current",
        "source": {
            "node_id": source_node_id,
        },
    });

    if let Some(rev) = rev {
        doc["_rev"] = serde_json::Value::String(rev);
    }

    db.put_document(&doc_id, &doc).await
        .context("Failed to write replica document")?;
    Ok(())
}

async fn flush_annotations(
    db: &CouchClient,
    node_id: &str,
    pending: &mut HashMap<String, HashMap<String, String>>,
) {
    if pending.is_empty() { return; }

    let snapshot: HashMap<String, HashMap<String, String>> = std::mem::take(pending);
    let mut batch: Vec<serde_json::Value> = Vec::new();

    for (file_id, targets) in &snapshot {
        let file_uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
        let doc_id = format!("annotation::{}::replication", file_uuid);

        // Get existing annotation doc for _rev
        let existing = db.get_document(&doc_id).await.ok();
        let rev = existing.as_ref().and_then(|d| d.get("_rev")).and_then(|v| v.as_str()).map(|s| s.to_string());

        let mut targets_data = serde_json::json!({});
        for (tname, status) in targets {
            targets_data[tname] = serde_json::json!({
                "status": status,
                "updated_at": Utc::now().to_rfc3339(),
            });
        }

        let mut doc = serde_json::json!({
            "_id": doc_id,
            "type": "annotation",
            "file_id": file_id,
            "node_id": node_id,
            "plugin_name": "replication",
            "status": "ok",
            "annotated_at": Utc::now().to_rfc3339(),
            "data": { "targets": targets_data },
        });

        if let Some(rev) = rev {
            doc["_rev"] = serde_json::Value::String(rev);
        }

        batch.push(doc);

        if batch.len() >= 50 {
            let _ = db.bulk_docs(&batch).await;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        let _ = db.bulk_docs(&batch).await;
    }
}

async fn sweep_deletion_log(
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();

    let to_purge: Vec<(String, String, String)> = {
        let guard = conn.lock().unwrap();
        let mut stmt = guard.prepare(
            "SELECT file_id, target_name, remote_key FROM deletion_log
             WHERE purged = 0 AND (retain_until IS NULL OR retain_until <= ?1)"
        )?;
        let rows = stmt.query_map([&now], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .flatten()
            .collect::<Vec<_>>();
        rows
    };

    let (_, targets) = load_rules_and_targets(db).await?;

    for (file_id, target_name, remote_key) in to_purge {
        let target = match targets.get(&target_name) { Some(t) => t, None => continue };

        if let Ok(adapter) = build_backend(target) {
            if adapter.delete(&remote_key).await.is_ok() {
                let guard = conn.lock().unwrap();
                guard.execute(
                    "UPDATE deletion_log SET purged = 1 WHERE file_id = ?1 AND target_name = ?2",
                    params![file_id, target_name],
                )?;
            }
        }
    }

    Ok(())
}

async fn run_full_scan(
    db: &CouchClient,
    conn: &Arc<std::sync::Mutex<Connection>>,
    node_id: &str,
    pending_annotations: &mut HashMap<String, HashMap<String, String>>,
    pending_uploads: &mut HashMap<(String, String), serde_json::Value>,
) -> anyhow::Result<()> {
    info!("Starting replication full scan");

    let (rules, targets) = load_rules_and_targets(db).await?;
    if rules.is_empty() { return Ok(()); }

    // Load all active files for this node
    let files_resp = db.all_docs_by_prefix("file::", true).await?;

    let mut matched_pairs: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for row in files_resp.rows {
        let file_doc = match row.doc { Some(d) => d, None => continue };
        if file_doc.get("type").and_then(|v| v.as_str()) != Some("file") { continue; }
        if file_doc.get("status").and_then(|v| v.as_str()) == Some("deleted") { continue; }

        let doc_node = file_doc.get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str()).unwrap_or("");
        if doc_node != node_id { continue; }

        let file_id = file_doc.get("_id").and_then(|v| v.as_str()).unwrap_or("");
        let file_uuid = file_id.strip_prefix("file::").unwrap_or(file_id);

        let ctx = build_step_context(db, file_id, file_uuid, conn).await;
        let parsed = parse_file_doc(&file_doc);

        for rule in &rules {
            if rule.source_node_id != "*" && rule.source_node_id != node_id { continue; }
            if let Some(ref prefix) = rule.source_path_prefix {
                let ep = file_doc.get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str()).unwrap_or("");
                if !ep.starts_with(prefix.as_str()) { continue; }
            }

            let result = evaluate_steps(&rule.steps, &parsed, file_id, &rule.default_result, &ctx);
            if result == StepResult::Include {
                matched_pairs.insert((file_id.to_string(), rule.target_name.clone()));

                // Queue if stale
                let existing = get_replication_state(conn, file_id, &rule.target_name);
                let source_mtime = file_doc.get("mtime").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let source_size = file_doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

                let needs_upload = existing.map_or(true, |s| s.source_mtime != source_mtime || s.source_size != source_size);
                if needs_upload {
                    pending_uploads.insert((file_id.to_string(), rule.target_name.clone()), file_doc.clone());
                }
            }
        }
    }

    // Check for files in manifest that no longer match any rule (un-replication)
    let all_replicated: Vec<(String, String)> = {
        let guard = conn.lock().unwrap();
        let mut stmt = guard.prepare("SELECT file_id, target_name FROM replication_state")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .flatten()
            .collect::<Vec<_>>();
        rows
    };

    for (file_id, target_name) in all_replicated {
        if !matched_pairs.contains(&(file_id.clone(), target_name.clone())) {
            check_un_replication(&file_id, &target_name, &targets, conn, pending_annotations).await;
        }
    }

    info!(
        matched = matched_pairs.len(),
        "Replication full scan complete"
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn load_rules_and_targets(
    db: &CouchClient,
) -> anyhow::Result<(Vec<ReplicationRule>, HashMap<String, ReplicationTarget>)> {
    // Load storage backends
    let backends_resp = db.all_docs_by_prefix("storage_backend::", true).await?;
    let mut targets: HashMap<String, ReplicationTarget> = HashMap::new();

    for row in backends_resp.rows {
        let doc = match row.doc { Some(d) => d, None => continue };
        if doc.get("type").and_then(|v| v.as_str()) != Some("storage_backend") { continue; }
        if doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) { continue; }
        if doc.get("mode").and_then(|v| v.as_str()) != Some("target") { continue; }

        let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let backend_type = doc.get("backend").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let prefix = doc.get("backend_config").and_then(|c| c.get("prefix")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let schedule = doc.get("schedule").and_then(|v| v.as_str()).and_then(parse_schedule);
        let bandwidth_limit_mbps = doc.get("bandwidth_limit_mbps").and_then(|v| v.as_i64()).map(|v| v as i32);
        let retention_days = doc.get("retention").and_then(|r| r.get("keep_deleted_days")).and_then(|v| v.as_i64()).unwrap_or(30) as i32;
        let remove_unmatched = doc.get("remove_unmatched").and_then(|v| v.as_bool()).unwrap_or(false);

        // Load credential document if referenced
        let credential_doc = if let Some(cref) = doc.get("credentials_ref").and_then(|v| v.as_str()) {
            db.get_document(&format!("credential::{}", cref)).await.ok()
        } else {
            None
        };

        targets.insert(name.clone(), ReplicationTarget {
            name,
            backend_type,
            prefix,
            schedule,
            bandwidth_limit_mbps,
            workers: 2,
            retention_days,
            remove_unmatched,
            backend_doc: doc,
            credential_doc,
        });
    }

    // Load replication rules
    let rules_resp = db.all_docs_by_prefix("replication_rule::", true).await?;
    let mut rules: Vec<ReplicationRule> = Vec::new();

    for row in rules_resp.rows {
        let doc = match row.doc { Some(d) => d, None => continue };
        if doc.get("type").and_then(|v| v.as_str()) != Some("replication_rule") { continue; }
        if doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) { continue; }

        let rule_id = doc.get("rule_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let target_name = doc.get("target_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let source_node_id = doc.get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let source_path_prefix = doc.get("source").and_then(|s| s.get("path_prefix")).and_then(|v| v.as_str()).map(|s| s.to_string());

        let steps: Vec<mosaicfs_common::documents::Step> = doc.get("steps")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let default_result = match doc.get("default_result").and_then(|v| v.as_str()).unwrap_or("exclude") {
            "include" => StepResult::Include,
            _ => StepResult::Exclude,
        };

        rules.push(ReplicationRule {
            rule_id,
            name,
            target_name,
            source_node_id,
            source_path_prefix,
            steps,
            default_result,
        });
    }

    Ok((rules, targets))
}

async fn build_step_context(
    db: &CouchClient,
    file_id: &str,
    file_uuid: &str,
    conn: &Arc<std::sync::Mutex<Connection>>,
) -> ReplicationStepContext {
    // Labels from label_assignment document
    let labels: std::collections::HashSet<String> = db
        .get_document(&format!("label_assignment::{}", file_uuid))
        .await
        .ok()
        .and_then(|d| d.get("labels").and_then(|v| v.as_array()).map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
        }))
        .unwrap_or_default();

    // Last access from access document
    let last_access: Option<DateTime<Utc>> = db
        .get_document(&format!("access::{}", file_id))
        .await
        .ok()
        .and_then(|d| d.get("last_access").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()));

    // Replicas from SQLite
    let replicas: Vec<(String, String)> = {
        let guard = conn.lock().unwrap();
        let mut stmt = match guard.prepare(
            "SELECT target_name, 'current' FROM replication_state WHERE file_id = ?"
        ) {
            Ok(s) => s,
            Err(_) => return ReplicationStepContext { labels, last_access, replicas: vec![] },
        };
        let rows = stmt.query_map([file_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        rows
    };

    ReplicationStepContext { labels, last_access, replicas }
}

struct ReplicationStateRow {
    source_mtime: String,
    source_size: u64,
    remote_key: String,
}

fn get_replication_state(
    conn: &Arc<std::sync::Mutex<Connection>>,
    file_id: &str,
    target_name: &str,
) -> Option<ReplicationStateRow> {
    let guard = conn.lock().unwrap();
    guard.query_row(
        "SELECT source_mtime, source_size, remote_key FROM replication_state WHERE file_id = ?1 AND target_name = ?2",
        params![file_id, target_name],
        |row| Ok(ReplicationStateRow {
            source_mtime: row.get(0)?,
            source_size: row.get::<_, i64>(1)? as u64,
            remote_key: row.get(2)?,
        }),
    ).ok()
}

fn queue_upload(conn: &Arc<std::sync::Mutex<Connection>>, file_id: &str, target_name: &str) {
    let guard = conn.lock().unwrap();
    guard.execute(
        "INSERT OR IGNORE INTO upload_queue (file_id, target_name, queued_at) VALUES (?1, ?2, ?3)",
        params![file_id, target_name, Utc::now().to_rfc3339()],
    ).unwrap_or_default();
}

fn update_annotation_status(
    pending: &mut HashMap<String, HashMap<String, String>>,
    file_id: &str,
    target_name: &str,
    status: &str,
) {
    pending
        .entry(file_id.to_string())
        .or_default()
        .insert(target_name.to_string(), status.to_string());
}

/// Parse a file doc into the minimal FileDocument struct needed by evaluate_steps.
fn parse_file_doc(doc: &serde_json::Value) -> mosaicfs_common::documents::FileDocument {
    use mosaicfs_common::documents::*;

    let source_node_id = doc.get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let export_path = doc.get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let export_parent = doc.get("source").and_then(|s| s.get("export_parent")).and_then(|v| v.as_str()).unwrap_or("").to_string();

    let status = if doc.get("status").and_then(|v| v.as_str()).unwrap_or("active") == "deleted" {
        FileStatus::Deleted
    } else {
        FileStatus::Active
    };

    FileDocument {
        doc_type: FileType::File,
        inode: doc.get("inode").and_then(|v| v.as_u64()).unwrap_or(0),
        name: doc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        source: FileSource {
            node_id: source_node_id,
            export_path,
            export_parent,
        },
        size: doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
        mtime: doc.get("mtime")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Utc::now),
        mime_type: doc.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
        status,
        deleted_at: None,
        migrated_from: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_schedule() {
        let (start, end) = parse_schedule("02:00-06:00").unwrap();
        assert_eq!(start.hour(), 2);
        assert_eq!(start.minute(), 0);
        assert_eq!(end.hour(), 6);

        // Midnight-wrapping
        let (start, end) = parse_schedule("22:00-04:00").unwrap();
        assert_eq!(start.hour(), 22);
        assert_eq!(end.hour(), 4);

        assert!(parse_schedule("invalid").is_none());
        assert!(parse_schedule("").is_none());
    }

    #[test]
    fn test_in_schedule_window_no_schedule() {
        // No schedule = always active
        assert!(in_schedule_window(None));
    }

    #[test]
    fn test_token_bucket_creation() {
        let tb = TokenBucket::new(10); // 10 Mbps
        assert!(tb.capacity_bytes > 0.0);
        assert!(tb.bytes_per_sec > 0.0);
    }
}
