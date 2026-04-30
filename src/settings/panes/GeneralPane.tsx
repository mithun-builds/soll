import { useEffect, useState, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

type Snapshot = {
  user_name: string;
  ai_cleanup_enabled: boolean;
  whisper_model: string;
  dictionary_count: number;
};

type Update = {
  user_name?: string;
  ai_cleanup_enabled?: boolean;
};

type UpdateCheck = {
  current: string;
  latest: string;
  update_available: boolean;
  release_url: string;
};

const DEFAULT_SHORTCUT = "Control+Shift+Space";

const MOD_GLYPH: Record<string, string> = {
  Control: "⌃",
  Ctrl: "⌃",
  Shift: "⇧",
  Alt: "⌥",
  Option: "⌥",
  Command: "⌘",
  Cmd: "⌘",
  Meta: "⌘",
  Super: "⌘",
};

// Verbose names match what macOS System Settings shows the user — "Option"
// rather than the HID-spec "Alt", "Command" rather than "Meta/Super".
const MOD_NAME: Record<string, string> = {
  Control: "Control",
  Ctrl: "Control",
  Shift: "Shift",
  Alt: "Option",
  Option: "Option",
  Command: "Command",
  Cmd: "Command",
  Meta: "Command",
  Super: "Command",
};

const KEY_GLYPH: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "↵",
  Return: "↵",
  Tab: "⇥",
  Escape: "⎋",
  Backspace: "⌫",
  Delete: "⌦",
  Space: "Space",
};

function stripKeyPrefix(part: string): string {
  if (part.startsWith("Key") && part.length === 4) return part.slice(3);
  if (part.startsWith("Digit") && part.length === 6) return part.slice(5);
  return part;
}

function prettyAccelerator(accel: string): string {
  return accel
    .split("+")
    .map((part) => MOD_GLYPH[part] ?? KEY_GLYPH[part] ?? stripKeyPrefix(part))
    .join("");
}

function verboseAccelerator(accel: string): string {
  return accel
    .split("+")
    .map((part) => MOD_NAME[part] ?? stripKeyPrefix(part))
    .join(" + ");
}

// Build a Tauri accelerator string from a browser keydown event. Returns null
// while the user is still holding modifiers without a "real" key, or when the
// combo lacks a non-Shift modifier (Shift-only would conflict with typing).
function accelFromEvent(e: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  if (!e.ctrlKey && !e.altKey && !e.metaKey) return null;

  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Command");

  if (!e.code) return null;
  return [...mods, e.code].join("+");
}

export function GeneralPane() {
  const [s, setS] = useState<Snapshot | null>(null);
  const [userNameDraft, setUserNameDraft] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
  const [err, setErr] = useState<string | null>(null);
  const [version, setVersion] = useState<string>("");
  const [checking, setChecking] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateCheck | null>(null);
  const [updateErr, setUpdateErr] = useState<string | null>(null);
  const [shortcut, setShortcut] = useState<string>("");
  const [capturing, setCapturing] = useState(false);
  const [shortcutErr, setShortcutErr] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const snap = await invoke<Snapshot>("settings_get");
      setS(snap);
      setUserNameDraft(snap.user_name);
    } catch (e) {
      setErr(String(e));
    }
  };

  useEffect(() => {
    refresh();
    invoke<string>("app_version").then(setVersion).catch(() => {});
    invoke<string>("get_shortcut").then(setShortcut).catch(() => {});
  }, []);

  // Capture a new shortcut while the user is in "Change" mode. Listener is
  // attached at window level (capture phase) so the keystrokes don't leak
  // into focused inputs like the name field, and so any modifier combo —
  // including ones that target browser/system actions — is intercepted.
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(false);
        setShortcutErr(null);
        return;
      }
      const accel = accelFromEvent(e);
      if (!accel) return;
      setCapturing(false);
      invoke<void>("set_shortcut", { accelerator: accel })
        .then(() => {
          setShortcut(accel);
          setShortcutErr(null);
        })
        .catch((err) => {
          setShortcutErr(String(err));
        });
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKey, { capture: true });
  }, [capturing]);

  const resetShortcut = async () => {
    try {
      await invoke<void>("set_shortcut", { accelerator: DEFAULT_SHORTCUT });
      setShortcut(DEFAULT_SHORTCUT);
      setShortcutErr(null);
    } catch (e) {
      setShortcutErr(String(e));
    }
  };

  const checkForUpdate = async () => {
    setChecking(true);
    setUpdateErr(null);
    try {
      const info = await invoke<UpdateCheck>("check_for_update");
      setUpdateInfo(info);
    } catch (e) {
      setUpdateErr(String(e));
    } finally {
      setChecking(false);
    }
  };

  const update = async (u: Update) => {
    setSaveState("saving");
    try {
      const snap = await invoke<Snapshot>("settings_set", { update: u });
      setS(snap);
      setErr(null);
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 1200);
    } catch (e) {
      setErr(String(e));
      setSaveState("idle");
    }
  };

  const onSaveText = async (e: FormEvent) => {
    e.preventDefault();
    await update({ user_name: userNameDraft });
  };

  if (!s) {
    return (
      <div className="pane-loading">{err ? `Error: ${err}` : "Loading…"}</div>
    );
  }

  return (
    <>
      {/* ── Identity ── */}
      <h1>Identity</h1>
      <p className="subtle">
        Your name appears in dictated emails and is available to custom
        skills as <code>[name]</code>.
      </p>

      <form className="pane-section" onSubmit={onSaveText}>
        <label>
          <span className="field-label">Your name</span>
          <input
            type="text"
            placeholder="e.g. Mithun"
            value={userNameDraft}
            onChange={(e) => setUserNameDraft(e.target.value)}
          />
        </label>

        <button
          type="submit"
          className="primary"
          disabled={saveState === "saving"}
        >
          {saveState === "saving"
            ? "Saving…"
            : saveState === "saved"
              ? "Saved ✓"
              : "Save"}
        </button>
      </form>

      {/* ── Hotkey ── */}
      <h1>Hotkey</h1>
      <p className="subtle">The shortcut you hold to start dictating.</p>

      <div className={`hotkey-row${capturing ? " hotkey-row--capturing" : ""}`}>
        <span className="subtle">Push-to-talk</span>
        <code>
          {capturing
            ? "Press a key combination…"
            : prettyAccelerator(shortcut || DEFAULT_SHORTCUT)}
        </code>
        {!capturing && (
          <span className="hotkey-verbose">
            {verboseAccelerator(shortcut || DEFAULT_SHORTCUT)}
          </span>
        )}
        {capturing ? (
          <button
            type="button"
            className="secondary"
            onClick={() => {
              setCapturing(false);
              setShortcutErr(null);
            }}
          >
            Cancel
          </button>
        ) : (
          <>
            {shortcut && shortcut !== DEFAULT_SHORTCUT && (
              <button
                type="button"
                className="secondary"
                onClick={resetShortcut}
              >
                Reset
              </button>
            )}
            <button
              type="button"
              className="secondary"
              onClick={() => {
                setShortcutErr(null);
                setCapturing(true);
              }}
            >
              Change
            </button>
          </>
        )}
      </div>
      {capturing && (
        <p className="subtle hint">
          Press the combination you'd like to use, or Esc to cancel.
        </p>
      )}
      {shortcutErr && !capturing && (
        <p className="subtle hint hotkey-error">{shortcutErr}</p>
      )}
      {!capturing && (
        <ul className="hotkey-notes subtle">
          <li>
            Must include at least one of ⌃, ⌥, or ⌘ — Shift alone would clash
            with typing.
          </li>
          <li>
            Combos owned by macOS (e.g. ⌘Space for Spotlight) will be rejected
            with an error here.
          </li>
          <li>
            Avoid common app shortcuts like ⌘V or ⌃C — Soll will register them
            globally and override those actions everywhere.
          </li>
          <li>
            You hold the shortcut while speaking, so pick keys that are
            comfortable to hold for a few seconds.
          </li>
        </ul>
      )}

      {/* ── About ── */}
      <h1>About</h1>
      <p className="subtle">
        The release you're on, and a one-click way to see if there's a newer one.
      </p>

      <div className="version-row">
        <span className="subtle">Version</span>
        <code>{version ? `v${version}` : "…"}</code>
        {updateInfo && !updateInfo.update_available && (
          <span className="subtle version-status">Up to date</span>
        )}
        {updateInfo && updateInfo.update_available && (
          <a
            href={updateInfo.release_url}
            target="_blank"
            rel="noreferrer"
            className="version-status version-status--available"
          >
            v{updateInfo.latest} available →
          </a>
        )}
        {updateErr && (
          <span className="subtle version-status">Couldn't check</span>
        )}
        <button
          type="button"
          className="secondary"
          onClick={checkForUpdate}
          disabled={checking}
        >
          {checking ? "Checking…" : "Check for updates"}
        </button>
      </div>
    </>
  );
}
