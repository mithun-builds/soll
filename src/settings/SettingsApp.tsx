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

export function SettingsApp() {
  const [s, setS] = useState<Snapshot | null>(null);
  const [userNameDraft, setUserNameDraft] = useState("");
  const [signOffDraft, setSignOffDraft] = useState("");
  const [saveState, setSaveState] = useState<
    "idle" | "saving" | "saved" | "error"
  >("idle");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const snap = await invoke<Snapshot>("settings_get");
        setS(snap);
        setUserNameDraft(snap.user_name);
        setSignOffDraft(snap.email_sign_off);
      } catch (e) {
        setErr(String(e));
      }
    })();
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
      setSaveState("error");
    }
  };

  const onSaveText = async (e: FormEvent) => {
    e.preventDefault();
    await update({
      user_name: userNameDraft,
      email_sign_off: signOffDraft,
    });
  };

  const onToggleAi = async () => {
    if (!s) return;
    await update({ ai_cleanup_enabled: !s.ai_cleanup_enabled });
  };

  if (err && !s) {
    return (
      <div className="settings">
        <div className="settings-error">Couldn't load settings: {err}</div>
      </div>
    );
  }
  if (!s) {
    return (
      <div className="settings">
        <div className="settings-loading">Loading…</div>
      </div>
    );
  }

  return (
    <div className="settings">
      <header className="settings-header">
        <h1>Settings</h1>
        <p className="subtle">
          Configure how Svara formats your dictations. Changes take effect
          immediately.
        </p>
      </header>

      <form className="settings-form" onSubmit={onSaveText}>
        <section className="settings-section">
          <label className="settings-label">
            <span>Your name</span>
            <span className="subtle hint">
              Appears at the bottom of dictated emails.
            </span>
          </label>
          <input
            type="text"
            placeholder="e.g. Mithun"
            value={userNameDraft}
            onChange={(e) => setUserNameDraft(e.target.value)}
          />
        </section>

        <section className="settings-section">
          <label className="settings-label">
            <span>Email sign-off</span>
            <span className="subtle hint">
              Word before your name. Common: Best, Thanks, Regards, Cheers.
            </span>
          </label>
          <input
            type="text"
            placeholder="Best"
            value={signOffDraft}
            onChange={(e) => setSignOffDraft(e.target.value)}
          />
        </section>

        <button
          type="submit"
          className="settings-save"
          disabled={saveState === "saving"}
        >
          {saveState === "saving"
            ? "Saving…"
            : saveState === "saved"
              ? "Saved ✓"
              : "Save"}
        </button>
      </form>

      <section className="settings-section">
        <div className="settings-toggle-row">
          <div>
            <div className="settings-title">AI cleanup (Ollama)</div>
            <div className="subtle hint">
              Polish grammar, remove fillers, add punctuation. When off, the
              raw Whisper transcript is pasted directly.
            </div>
          </div>
          <button
            type="button"
            className={`settings-toggle ${s.ai_cleanup_enabled ? "on" : "off"}`}
            onClick={onToggleAi}
            aria-pressed={s.ai_cleanup_enabled}
          >
            <span className="settings-toggle-dot" />
          </button>
        </div>
      </section>

      <section className="settings-section settings-readonly">
        <div className="settings-row">
          <span className="subtle">Whisper model</span>
          <code>{s.whisper_model}</code>
        </div>
        <div className="settings-row">
          <span className="subtle">Dictionary entries</span>
          <code>{s.dictionary_count}</code>
        </div>
        <div className="settings-row">
          <span className="subtle">Hotkey</span>
          <code>⌃⇧Space</code>
        </div>
        <p className="subtle hint">
          Change model or dictionary from the tray menu. Hotkey rebinding is
          on the roadmap.
        </p>
      </section>

      <section className="settings-section">
        <h2>Email mode</h2>
        <p className="subtle">
          Start a dictation with <em>"email to [name]…"</em> to auto-format
          as a short email with greeting and sign-off.
        </p>
        <div className="settings-example">
          <strong>You say:</strong>{" "}
          <em>"email to Jane can we push the launch to Friday"</em>
          <br />
          <strong>Svara pastes:</strong>
          <pre>
{`Hi Jane,

Can we push the launch to Friday?

${signOffDraft || s.email_sign_off},${userNameDraft || s.user_name ? `\n${userNameDraft || s.user_name}` : ""}`}
          </pre>
        </div>
      </section>

      {err && <div className="settings-error">{err}</div>}
    </div>
  );
}
