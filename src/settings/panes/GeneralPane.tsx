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
      <h1>General</h1>
      <p className="subtle">Your identity and hotkey — AI settings live in Models.</p>

      <form className="pane-section" onSubmit={onSaveText}>
        <label>
          <span className="field-label">Your name</span>
          <span className="subtle hint">
            Appears at the bottom of dictated emails and can be referenced in
            custom skills.
          </span>
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

      <div className="pane-section readonly-grid">
        <div className="readonly-row">
          <span className="subtle">Hotkey</span>
          <code>⌃⇧Space</code>
        </div>
        <p className="subtle hint">
          Email sign-off now lives inside the Email skill's output template.
          Change it in <strong>Skills → email → Edit markdown</strong>.
          Hotkey rebinding is planned for a future update.
        </p>
      </div>

      <div className="pane-section version-row">
        <span className="subtle">Version</span>
        <code>{version ? `v${version}` : "…"}</code>
        <button
          type="button"
          className="secondary"
          onClick={checkForUpdate}
          disabled={checking}
        >
          {checking ? "Checking…" : "Check for updates"}
        </button>
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
      </div>
    </>
  );
}
