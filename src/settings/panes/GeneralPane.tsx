import { useEffect, useState, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

type Snapshot = {
  user_name: string;
  ai_cleanup_enabled: boolean;
  email_sign_off: string;
  whisper_model: string;
  dictionary_count: number;
};

type Update = {
  user_name?: string;
  ai_cleanup_enabled?: boolean;
  email_sign_off?: string;
};

export function GeneralPane() {
  const [s, setS] = useState<Snapshot | null>(null);
  const [userNameDraft, setUserNameDraft] = useState("");
  const [signOffDraft, setSignOffDraft] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
  const [err, setErr] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const snap = await invoke<Snapshot>("settings_get");
      setS(snap);
      setUserNameDraft(snap.user_name);
      setSignOffDraft(snap.email_sign_off);
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
    await update({ user_name: userNameDraft, email_sign_off: signOffDraft });
  };

  if (!s) {
    return (
      <div className="pane-loading">{err ? `Error: ${err}` : "Loading…"}</div>
    );
  }

  return (
    <>
      <h1>General</h1>
      <p className="subtle">Identity + AI behavior across every dictation.</p>

      <form className="pane-section" onSubmit={onSaveText}>
        <label>
          <span className="field-label">Your name</span>
          <span className="subtle hint">
            Appears at the bottom of dictated emails.
          </span>
          <input
            type="text"
            placeholder="e.g. Mithun"
            value={userNameDraft}
            onChange={(e) => setUserNameDraft(e.target.value)}
          />
        </label>

        <label>
          <span className="field-label">Email sign-off</span>
          <span className="subtle hint">
            Word before your name. Common: Best, Thanks, Regards, Cheers.
          </span>
          <input
            type="text"
            placeholder="Best"
            value={signOffDraft}
            onChange={(e) => setSignOffDraft(e.target.value)}
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

      <div className="pane-section toggle-row">
        <div>
          <div className="field-label">AI cleanup (Ollama)</div>
          <div className="subtle hint">
            Polish grammar, remove fillers, add punctuation. When off, raw
            Whisper output is pasted.
          </div>
        </div>
        <button
          type="button"
          className={`toggle ${s.ai_cleanup_enabled ? "on" : "off"}`}
          onClick={() => update({ ai_cleanup_enabled: !s.ai_cleanup_enabled })}
          aria-pressed={s.ai_cleanup_enabled}
        >
          <span className="toggle-dot" />
        </button>
      </div>

      <div className="pane-section readonly-grid">
        <div className="readonly-row">
          <span className="subtle">Hotkey</span>
          <code>⌃⇧Space</code>
        </div>
        <p className="subtle hint">
          Hotkey rebinding is planned for a future update.
        </p>
      </div>
    </>
  );
}
