//! Unified MosaicFS binary.
//!
//! Loads `/etc/mosaicfs/mosaicfs.toml` (or the `--config` path), inspects
//! `[features]`, and starts only the subsystems enabled there. Each node
//! decides its role via config — a NAS enables `agent + web_ui`, a
//! laptop enables `agent + vfs`, a headless indexer enables `agent`
//! alone.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mosaicfs_agent::start_agent;
use mosaicfs_common::config::MosaicfsConfig;
use mosaicfs_common::secrets::{self, SecretsBackend};
use mosaicfs_server::{run_bootstrap, start_web_ui};
#[cfg(feature = "vfs")]
use mosaicfs_vfs::start_vfs;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_PATH: &str = "/etc/mosaicfs/mosaicfs.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("bootstrap") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("warn"))
            .init();
        let (cfg, config_path) = load_config(&args)?;
        let secrets = secrets::open(&cfg, Some(&config_path))?;
        let json_output = args.iter().any(|a| a == "--json");
        return run_bootstrap(&cfg, secrets, json_output).await;
    }

    if args.get(1).map(|s| s.as_str()) == Some("secrets") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("warn"))
            .init();
        let (cfg, config_path) = load_config(&args)?;
        let secrets = secrets::open(&cfg, Some(&config_path))?;
        return run_secrets_subcommand(&args, &*secrets, &config_path);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let (cfg, config_path) = load_config(&args)?;
    let cfg = Arc::new(cfg);
    let secrets = secrets::open(&cfg, Some(&config_path))?;
    info!(
        agent = cfg.features.agent,
        vfs = cfg.features.vfs,
        web_ui = cfg.features.web_ui,
        secrets_backend = secrets.kind(),
        "mosaicfs starting"
    );

    let mut set: tokio::task::JoinSet<(&'static str, anyhow::Result<()>)> = tokio::task::JoinSet::new();

    if cfg.features.web_ui {
        let cfg = Arc::clone(&cfg);
        let secrets = Arc::clone(&secrets);
        set.spawn(async move { ("web_ui", start_web_ui(cfg, secrets).await) });
    }
    if cfg.features.agent {
        let cfg = Arc::clone(&cfg);
        let secrets = Arc::clone(&secrets);
        set.spawn(async move { ("agent", start_agent(cfg, secrets).await) });
    }
    #[cfg(feature = "vfs")]
    if cfg.features.vfs {
        let cfg = Arc::clone(&cfg);
        set.spawn(async move { ("vfs", start_vfs(cfg).await) });
    }

    if set.is_empty() {
        anyhow::bail!("no features enabled — set at least one of agent/vfs/web_ui");
    }

    // Wait until the first subsystem exits. If any subsystem dies, exit
    // the whole binary so the supervisor (podman, systemd, launchd)
    // restarts the pod rather than leaving half of it running silently.
    if let Some(join) = set.join_next().await {
        match join {
            Ok((name, Ok(()))) => info!(subsystem = name, "subsystem exited cleanly"),
            Ok((name, Err(e))) => error!(subsystem = name, error = %e, "subsystem returned error"),
            Err(e) => error!(error = %e, "subsystem task panicked"),
        }
    }
    set.shutdown().await;
    Ok(())
}

fn load_config(args: &[String]) -> anyhow::Result<(MosaicfsConfig, PathBuf)> {
    let path = PathBuf::from(
        arg_value(args, "--config").unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string()),
    );
    let cfg = MosaicfsConfig::load(&path)?;
    Ok((cfg, path))
}

/// Dispatch `mosaicfs secrets [list|get NAME|import]`.
fn run_secrets_subcommand(
    args: &[String],
    secrets: &dyn SecretsBackend,
    config_path: &Path,
) -> anyhow::Result<()> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
    let assume_yes = args.iter().any(|a| a == "--yes" || a == "-y");
    match sub {
        "list" => secrets_list(secrets),
        "get" => {
            let name = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("usage: mosaicfs secrets get NAME [--yes]"))?;
            secrets_get(secrets, name, assume_yes)
        }
        "import" => secrets_import(secrets, config_path, assume_yes),
        "" => {
            eprintln!("usage: mosaicfs secrets <list|get NAME|import> [--yes]");
            std::process::exit(2);
        }
        other => {
            eprintln!(
                "unknown subcommand: secrets {other} (expected list, get, or import)"
            );
            std::process::exit(2);
        }
    }
}

fn secrets_list(secrets: &dyn SecretsBackend) -> anyhow::Result<()> {
    let present = secrets.list()?;
    println!("backend: {}", secrets.kind());
    if present.is_empty() {
        println!("(no secrets present)");
        return Ok(());
    }
    for name in present {
        println!("{name}");
    }
    Ok(())
}

/// Print a single secret to stdout. Gated behind a `[y/N]` confirmation
/// unless `--yes` is passed — `get` is a recovery tool, not a routine
/// read path.
fn secrets_get(
    secrets: &dyn SecretsBackend,
    name: &str,
    assume_yes: bool,
) -> anyhow::Result<()> {
    if !assume_yes {
        eprint!(
            "This will print the value of '{name}' to stdout. Continue? [y/N] "
        );
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        let answer = line.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("aborted");
            std::process::exit(1);
        }
    }
    let value = secrets.get(name)?;
    println!("{value}");
    Ok(())
}

/// Migrate every non-empty inline field from the TOML file into the
/// active (non-inline) backend, then offer to blank those fields.
fn secrets_import(
    secrets: &dyn SecretsBackend,
    config_path: &Path,
    assume_yes: bool,
) -> anyhow::Result<()> {
    if secrets.kind() == "inline" {
        anyhow::bail!(
            "active backend is \"inline\" — set [secrets].manager to another backend \
             before running `secrets import`"
        );
    }
    let inline = mosaicfs_common::secrets::read_inline_from_file(config_path)?;
    if inline.is_empty() {
        println!("no inline secret values found in {}", config_path.display());
        return Ok(());
    }

    println!("Importing {} secret(s) into {}:", inline.len(), secrets.kind());
    for (name, _) in &inline {
        println!("  {name}");
        secrets.set(name, &inline.iter().find(|(n, _)| n == name).unwrap().1)?;
    }
    println!("Import complete.");

    let proceed = if assume_yes {
        true
    } else {
        eprint!(
            "Blank these fields in {} so they no longer contain plaintext? [y/N] ",
            config_path.display()
        );
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        let answer = line.trim().to_lowercase();
        answer == "y" || answer == "yes"
    };

    if proceed {
        let names: Vec<&str> = inline.iter().map(|(n, _)| n.as_str()).collect();
        mosaicfs_common::secrets::blank_inline_in_file(config_path, &names)?;
        println!("Inline fields blanked.");
    } else {
        println!("Left the file untouched. Edit it manually to remove plaintext values.");
    }
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(rest) = a.strip_prefix(&format!("{}=", name)) {
            return Some(rest.to_string());
        }
    }
    None
}
