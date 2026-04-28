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
  }, []);

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

      <div className="pane-section readonly-grid">
        <div className="readonly-row">
          <span className="subtle">Push-to-talk</span>
          <code>⌃⇧Space</code>
        </div>
        <p className="subtle hint">
          Hotkey rebinding is planned for a future update.
        </p>
      </div>

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
