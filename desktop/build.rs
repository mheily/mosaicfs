fn main() {
    // tauri-build validates externalBin files exist, so we must stage the
    // triple-named copy BEFORE calling tauri_build::build().
    //
    // For `cargo tauri build`: beforeBuildCommand in tauri.conf.json builds
    // ../target/release/mosaicfs first; this copy step then creates the
    // triple-named file that the bundler picks up.
    //
    // For plain `cargo build` (dev): if no release binary exists yet we write
    // an empty placeholder so tauri-build doesn't error. The real binary is
    // found next to the executable at runtime, so the placeholder is never run.
    let target = std::env::var("TARGET").unwrap_or_default();
    let src = "../target/release/mosaicfs";
    let dst = format!("../target/release/mosaicfs-{target}");

    std::fs::create_dir_all("../target/release").ok();
    println!("cargo:rerun-if-changed={src}");

    if std::path::Path::new(src).exists() {
        if let Err(e) = std::fs::copy(src, &dst) {
            println!("cargo:warning=could not stage server binary: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&dst) {
                    let mut perms = meta.permissions();
                    perms.set_mode(perms.mode() | 0o111);
                    let _ = std::fs::set_permissions(&dst, perms);
                }
            }
        }
    } else if !std::path::Path::new(&dst).exists() {
        std::fs::write(&dst, b"").ok();
        println!(
            "cargo:warning=server binary not found at {src} — \
             placeholder written; run `cargo build --release \
             --no-default-features --bin mosaicfs` before bundling"
        );
    }

    tauri_build::build();
}
