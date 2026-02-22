//! Filesystem watcher using `notify`.
//!
//! Watches configured paths for changes after the initial crawl.
//! Features:
//! - 500ms debounce per path
//! - Rename correlation (single update, not delete+create)
//! - Event storm throttling: switch to full crawl if >1000 events/sec for 5s

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// A filesystem change event (after debouncing and correlation).
#[derive(Debug, Clone)]
pub enum FsEvent {
    /// File was created or modified.
    Modified(PathBuf),
    /// File was deleted.
    Deleted(PathBuf),
    /// File was renamed from `old` to `new`.
    Renamed { from: PathBuf, to: PathBuf },
    /// Event storm detected — caller should trigger a full crawl.
    StormDetected,
}

/// Configuration for the watcher.
pub struct WatcherConfig {
    pub watch_paths: Vec<PathBuf>,
    pub excluded_paths: Vec<PathBuf>,
    pub debounce_ms: u64,
    pub storm_threshold: u64,     // events/sec
    pub storm_duration_secs: u64, // consecutive seconds
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            watch_paths: Vec::new(),
            excluded_paths: Vec::new(),
            debounce_ms: 500,
            storm_threshold: 1000,
            storm_duration_secs: 5,
        }
    }
}

/// Start the filesystem watcher. Returns a channel receiver for events
/// and a shutdown flag.
pub fn start_watcher(
    config: WatcherConfig,
) -> anyhow::Result<(mpsc::UnboundedReceiver<FsEvent>, Arc<AtomicBool>)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let (notify_tx, notify_rx) = std::sync::mpsc::channel();

    let mut watcher: RecommendedWatcher =
        notify::Watcher::new(notify_tx, notify::Config::default())?;

    for path in &config.watch_paths {
        if path.exists() {
            match watcher.watch(path, RecursiveMode::Recursive) {
                Ok(()) => info!(path = %path.display(), "Watching path"),
                Err(e) => warn!(path = %path.display(), error = %e, "Failed to watch path"),
            }
        } else {
            warn!(path = %path.display(), "Watch path does not exist, skipping");
        }
    }

    let excluded = config.excluded_paths.clone();
    let debounce_duration = Duration::from_millis(config.debounce_ms);
    let storm_threshold = config.storm_threshold;
    let storm_duration = config.storm_duration_secs;

    std::thread::Builder::new()
        .name("mosaicfs-watcher".to_string())
        .spawn(move || {
            let _watcher = watcher; // Keep watcher alive

            let mut debounce_map: HashMap<PathBuf, (Instant, EventKind)> = HashMap::new();
            let _rename_from: Option<(PathBuf, Instant)> = None;
            let mut event_count = 0u64;
            let mut count_window_start = Instant::now();
            let mut storm_seconds = 0u64;

            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    info!("Watcher shutting down");
                    break;
                }

                match notify_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(event)) => {
                        // Storm detection
                        event_count += 1;
                        let elapsed = count_window_start.elapsed();
                        if elapsed >= Duration::from_secs(1) {
                            let rate = event_count;
                            if rate > storm_threshold {
                                storm_seconds += 1;
                                if storm_seconds >= storm_duration {
                                    warn!(
                                        rate,
                                        consecutive_seconds = storm_seconds,
                                        "Event storm detected, triggering full crawl"
                                    );
                                    let _ = tx.send(FsEvent::StormDetected);
                                    storm_seconds = 0;
                                }
                            } else {
                                storm_seconds = 0;
                            }
                            event_count = 0;
                            count_window_start = Instant::now();
                        }

                        for path in event.paths.iter() {
                            if is_excluded(path, &excluded) {
                                continue;
                            }

                            match event.kind {
                                EventKind::Create(_) | EventKind::Modify(_) => {
                                    debounce_map.insert(
                                        path.clone(),
                                        (Instant::now(), event.kind),
                                    );
                                }
                                EventKind::Remove(_) => {
                                    debounce_map.insert(
                                        path.clone(),
                                        (Instant::now(), event.kind),
                                    );
                                }
                                EventKind::Access(_) => {
                                    // Ignore access events
                                }
                                _ => {
                                    // For rename events, try to correlate
                                    if matches!(event.kind, EventKind::Other) {
                                        // Platform-specific rename handling
                                        debounce_map.insert(
                                            path.clone(),
                                            (Instant::now(), event.kind),
                                        );
                                    }
                                }
                            }
                        }

                        // Handle rename pairs from notify
                        if event.paths.len() == 2
                            && matches!(
                                event.kind,
                                EventKind::Modify(notify::event::ModifyKind::Name(_))
                            )
                        {
                            let from = event.paths[0].clone();
                            let to = event.paths[1].clone();
                            debounce_map.remove(&from);
                            debounce_map.remove(&to);
                            let _ = tx.send(FsEvent::Renamed { from, to });
                        }
                    }
                    Ok(Err(e)) => {
                        error!(error = %e, "Watch error");
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        info!("Watcher channel disconnected");
                        break;
                    }
                }

                // Flush debounced events
                let now = Instant::now();
                let mut flushed = Vec::new();
                for (path, (timestamp, kind)) in &debounce_map {
                    if now.duration_since(*timestamp) >= debounce_duration {
                        let event = match kind {
                            EventKind::Remove(_) => FsEvent::Deleted(path.clone()),
                            _ => FsEvent::Modified(path.clone()),
                        };
                        let _ = tx.send(event);
                        flushed.push(path.clone());
                    }
                }
                for path in flushed {
                    debounce_map.remove(&path);
                }
            }
        })?;

    Ok((rx, shutdown))
}

fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded.iter().any(|excl| path.starts_with(excl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded() {
        let excluded = vec![
            PathBuf::from("/home/user/.cache"),
            PathBuf::from("/tmp"),
        ];
        assert!(is_excluded(Path::new("/home/user/.cache/foo"), &excluded));
        assert!(is_excluded(Path::new("/tmp/bar"), &excluded));
        assert!(!is_excluded(Path::new("/home/user/docs/file.txt"), &excluded));
    }

    #[test]
    fn test_watcher_config_defaults() {
        let config = WatcherConfig::default();
        assert_eq!(config.debounce_ms, 500);
        assert_eq!(config.storm_threshold, 1000);
        assert_eq!(config.storm_duration_secs, 5);
    }

    #[test]
    fn test_fs_event_variants() {
        let modified = FsEvent::Modified(PathBuf::from("/test/file.txt"));
        let deleted = FsEvent::Deleted(PathBuf::from("/test/old.txt"));
        let renamed = FsEvent::Renamed {
            from: PathBuf::from("/test/old.txt"),
            to: PathBuf::from("/test/new.txt"),
        };
        let storm = FsEvent::StormDetected;

        // Just verify they can be created (type checking)
        match modified {
            FsEvent::Modified(p) => assert_eq!(p.to_str().unwrap(), "/test/file.txt"),
            _ => panic!("wrong variant"),
        }
        match deleted {
            FsEvent::Deleted(p) => assert_eq!(p.to_str().unwrap(), "/test/old.txt"),
            _ => panic!("wrong variant"),
        }
        match renamed {
            FsEvent::Renamed { from, to } => {
                assert_eq!(from.to_str().unwrap(), "/test/old.txt");
                assert_eq!(to.to_str().unwrap(), "/test/new.txt");
            }
            _ => panic!("wrong variant"),
        }
        match storm {
            FsEvent::StormDetected => {}
            _ => panic!("wrong variant"),
        }
    }
}
