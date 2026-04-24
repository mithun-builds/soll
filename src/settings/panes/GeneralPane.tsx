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

export function GeneralPane() {
  const [s, setS] = useState<Snapshot | null>(null);
  const [userNameDraft, setUserNameDraft] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
  const [err, setErr] = useState<string | null>(null);

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
  }, []);

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
    </>
  );
}
