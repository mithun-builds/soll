use anyhow::{Context, Result};
use std::thread;
use std::time::Duration;

/// Puts `text` on the clipboard and sends Cmd+V to the focused app.
///
/// We deliberately do NOT read the previous clipboard contents to restore
/// them later. On macOS 15+, calling `arboard::get_text()` when the
/// pasteboard holds a file URL (e.g. the user copied a file from Finder
/// inside ~/Documents) is treated as a Files & Folders access and triggers
/// a "Soll would like to access files in your Documents folder" prompt the
/// first time it runs. The user-visible cost of skipping the restore is
/// small — the dictated text replaces whatever was last copied, which is
/// what every other dictation tool does.
pub fn paste_text(text: &str) -> Result<()> {
    let mut cb = arboard::Clipboard::new().context("open clipboard")?;
    cb.set_text(text.to_string())
        .context("set clipboard text")?;

    // Tiny delay — gives macOS pasteboard time to settle before the keystroke.
    thread::sleep(Duration::from_millis(40));

    #[cfg(target_os = "macos")]
    send_cmd_v_macos()?;

    Ok(())
}

/// Synthesise Cmd+V via CoreGraphics CGEventPost — no Apple Events involved,
/// so macOS doesn't show the "Soll wants to control System Events" Automation
/// prompt on first use. Only requires Accessibility, which the onboarding
/// already grants in step 3.
#[cfg(target_os = "macos")]
fn send_cmd_v_macos() -> Result<()> {
    use std::os::raw::c_void;

    type CGEventTapLocation = u32;
    type CGKeyCode = u16;
    type CGEventFlags = u64;
    type CGEventSourceStateID = i32;

    const KCG_EVENT_SOURCE_STATE_HID_SYSTEM: CGEventSourceStateID = 1;
    const KCG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const KCG_EVENT_FLAG_COMMAND: CGEventFlags = 0x0010_0000;
    const KEY_V: CGKeyCode = 0x09; // ANSI 'v' virtual keycode

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state: CGEventSourceStateID) -> *mut c_void;
        fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            keycode: CGKeyCode,
            keydown: bool,
        ) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: CGEventFlags);
        fn CGEventPost(tap: CGEventTapLocation, event: *mut c_void);
        fn CFRelease(cf: *mut c_void);
    }

    unsafe {
        let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM);
        if source.is_null() {
            anyhow::bail!("CGEventSourceCreate returned null");
        }

        let down = CGEventCreateKeyboardEvent(source, KEY_V, true);
        if down.is_null() {
            CFRelease(source);
            anyhow::bail!("CGEventCreateKeyboardEvent (down) returned null");
        }
        CGEventSetFlags(down, KCG_EVENT_FLAG_COMMAND);
        CGEventPost(KCG_HID_EVENT_TAP, down);
        CFRelease(down);

        let up = CGEventCreateKeyboardEvent(source, KEY_V, false);
        if up.is_null() {
            CFRelease(source);
            anyhow::bail!("CGEventCreateKeyboardEvent (up) returned null");
        }
        CGEventSetFlags(up, KCG_EVENT_FLAG_COMMAND);
        CGEventPost(KCG_HID_EVENT_TAP, up);
        CFRelease(up);

        CFRelease(source);
    }
    Ok(())
}
