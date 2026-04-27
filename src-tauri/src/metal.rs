//! Point whisper.cpp at the bundled Metal shader so GPU acceleration actually
//! initializes at runtime.
//!
//! Context: whisper.cpp looks for `default.metallib` and `ggml-metal.metal` in:
//!   1. The macOS .app bundle's Resources dir (populated for distributed builds).
//!   2. `$GGML_METAL_PATH_RESOURCES` (explicit env var).
//!   3. The current working directory.
//!
//! In `cargo run` and `cargo run --example` neither #1 nor #3 finds the file,
//! so Metal init fails silently with an `NSCocoaErrorDomain Code=260` log and
//! whisper falls back to CPU inference. The fallback is invisible to callers;
//! only the log reveals it, and only if you're looking.
//!
//! We fix this by shipping `ggml-metal.metal` under `src-tauri/resources/` and
//! setting the env var to that path before whisper loads.

/// Set `GGML_METAL_PATH_RESOURCES` to the directory containing the bundled
/// `ggml-metal.metal` shader. Call once at process startup, before constructing
/// any `WhisperContext`.
#[cfg(target_os = "macos")]
pub fn ensure_metal_resources() {
    if std::env::var_os("GGML_METAL_PATH_RESOURCES").is_some() {
        return;
    }

    // 1) Installed .app bundle: shader lives at Soll.app/Contents/Resources/.
    //    Reading from there is uncontroversial — no TCC prompt.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents_dir) = exe.parent().and_then(|p| p.parent()) {
            let resources = contents_dir.join("Resources");
            let shader = resources.join("ggml-metal.metal");
            if shader.exists() {
                std::env::set_var("GGML_METAL_PATH_RESOURCES", &resources);
                log::info!("metal: bundle resources = {}", resources.display());
                return;
            }
        }
    }

    // 2) Dev fallback (cargo / tauri dev only). Don't probe this in release —
    //    `CARGO_MANIFEST_DIR` is the build path, which is often inside
    //    ~/Documents and triggers a Files & Folders TCC prompt the moment we
    //    stat it. Gating on `debug_assertions` keeps this path off in
    //    production binaries.
    #[cfg(debug_assertions)]
    {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let resources = std::path::Path::new(manifest).join("resources");
        let shader = resources.join("ggml-metal.metal");
        if shader.exists() {
            std::env::set_var("GGML_METAL_PATH_RESOURCES", &resources);
            log::info!("metal: dev resources = {}", resources.display());
            return;
        }
    }

    log::warn!("metal: ggml-metal.metal not found; whisper falls back to CPU");
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_metal_resources() {}
