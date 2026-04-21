use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Puts `text` on the clipboard and sends Cmd+V to the focused app.
/// Restores the previous clipboard contents after a short delay.
pub fn paste_text(text: &str) -> Result<()> {
    let mut cb = arboard::Clipboard::new().context("open clipboard")?;
    let prev = cb.get_text().ok();
    cb.set_text(text.to_string())
        .context("set clipboard text")?;

    // Tiny delay — gives macOS pasteboard time to settle before the keystroke.
    thread::sleep(Duration::from_millis(40));

    #[cfg(target_os = "macos")]
    send_cmd_v_macos()?;

    // Restore previous clipboard after a moment (don't clobber user's copied value).
    if let Some(p) = prev {
        let cb_reset = arboard::Clipboard::new().ok();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            if let Some(mut c) = cb_reset {
                let _ = c.set_text(p);
            }
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn send_cmd_v_macos() -> Result<()> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to keystroke "v" using command down"#)
        .status()
        .context("spawn osascript")?;
    if !status.success() {
        anyhow::bail!("osascript paste failed with {status}");
    }
    Ok(())
}
