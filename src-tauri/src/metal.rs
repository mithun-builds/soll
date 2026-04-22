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
    let manifest = env!("CARGO_MANIFEST_DIR");
    let resources = std::path::Path::new(manifest).join("resources");
    let shader = resources.join("ggml-metal.metal");
    if shader.exists() {
        std::env::set_var("GGML_METAL_PATH_RESOURCES", &resources);
        log::info!(
            "metal: GGML_METAL_PATH_RESOURCES={}",
            resources.display()
        );
    } else {
        log::warn!(
            "metal: ggml-metal.metal not at {}; whisper will fall back to CPU",
            shader.display()
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_metal_resources() {}
