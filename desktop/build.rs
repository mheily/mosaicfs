use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(&manifest_dir).parent().unwrap().to_path_buf();
    let target = std::env::var("TARGET").unwrap_or_default();
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());

    let release_bin = workspace_root.join("target/release/mosaicfs");
    let triple_bin = workspace_root.join(format!("target/release/mosaicfs-{target}"));

    // For release builds, compile the server binary from the workspace root.
    // We skip VFS (--no-default-features) so fuser/macFUSE is not required.
    if profile == "release" {
        let status = std::process::Command::new("cargo")
            .args([
                "build", "--release",
                "--no-default-features",
                "--bin", "mosaicfs",
            ])
            .current_dir(&workspace_root)
            .status()
            .expect("failed to invoke cargo for mosaicfs");

        if !status.success() {
            panic!("mosaicfs server binary build failed");
        }
    }

    // tauri-build validates that the externalBin triple-named file exists before
    // bundling. Stage the copy (or a placeholder for dev builds) before calling it.
    std::fs::create_dir_all(workspace_root.join("target/release")).ok();
    println!("cargo:rerun-if-changed={}", release_bin.display());

    if release_bin.exists() {
        if let Err(e) = std::fs::copy(&release_bin, &triple_bin) {
            println!("cargo:warning=could not stage server binary: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&triple_bin) {
                    let mut perms = meta.permissions();
                    perms.set_mode(perms.mode() | 0o111);
                    let _ = std::fs::set_permissions(&triple_bin, perms);
                }
            }
        }
    } else if !triple_bin.exists() {
        // Placeholder so tauri-build doesn't error on dev builds before
        // the server binary has been compiled for the first time.
        std::fs::write(&triple_bin, b"").ok();
    }

    tauri_build::build();
}
