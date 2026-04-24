import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type WhisperModel = {
  id: string;
  label: string;
  size: string;
  is_cached: boolean;
  is_active: boolean;
  is_downloading: boolean;
};

type OllamaModel = {
  tag: string;
  display_name: string;
  author: string;
  size: string;
  is_active: boolean;
  is_pulled: boolean;
};

type Snapshot = {
  ai_cleanup_enabled: boolean;
};

export function ModelsPane() {
  const [whisperModels, setWhisperModels] = useState<WhisperModel[]>([]);
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([]);
  const [aiOn, setAiOn] = useState(true);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [wList, oList, snap] = await Promise.all([
        invoke<WhisperModel[]>("models_list"),
        invoke<OllamaModel[]>("ollama_models_list"),
        invoke<Snapshot>("settings_get"),
      ]);
      setWhisperModels(wList);
      setOllamaModels(oList);
      setAiOn(snap.ai_cleanup_enabled);
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    // Poll while a download is in flight so progress + cache state update.
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  const activateWhisper = async (id: string) => {
    try {
      await invoke("model_activate", { id });
      refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const downloadWhisper = async (id: string, label: string, size: string) => {
    const ok = window.confirm(
      `Download ${label} (${size})?\n\nRuns in the background. You can keep dictating with your current model while it fetches.`,
    );
    if (!ok) return;
    try {
      await invoke("model_download", { id });
      refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const activateOllama = async (tag: string) => {
    try {
      await invoke("ollama_model_set", { tag });
      refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const toggleAiCleanup = async () => {
    try {
      await invoke("settings_set", {
        update: { ai_cleanup_enabled: !aiOn },
      });
      setAiOn((v) => !v);
    } catch (e) {
      setErr(String(e));
    }
  };

  const cached = whisperModels.filter((m) => m.is_cached);
  const uncached = whisperModels.filter((m) => !m.is_cached);

  return (
    <>
      {/* ── Whisper (speech-to-text) ── */}
      <h1>Whisper model</h1>
      <p className="subtle">
        Bigger models are more accurate but slower. Click any cached model to
        activate it.
      </p>

      <div className="pane-section">
        <h2>Available</h2>
        {cached.length === 0 ? (
          <div className="empty-hint">
            Nothing cached yet — download a model below.
          </div>
        ) : (
          <ul className="row-list">
            {cached.map((m) => (
              <li
                key={m.id}
                className={`row selectable ${m.is_active ? "active" : ""}`}
                onClick={() => !m.is_active && activateWhisper(m.id)}
              >
                <div className="row-title">
                  {m.is_active ? "✓ " : ""}
                  {m.label}{" "}
                  <span className="subtle">({m.size})</span>
                </div>
                {m.is_active ? (
                  <span className="badge">active</span>
                ) : (
                  <span className="row-hint subtle">Click to activate</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      {uncached.length > 0 && (
        <div className="pane-section">
          <h2>Available to download</h2>
          <ul className="row-list">
            {uncached.map((m) => (
              <li key={m.id} className="row">
                <div className="row-title">
                  {m.label}{" "}
                  <span className="subtle">({m.size})</span>
                </div>
                <button
                  type="button"
                  className="secondary"
                  disabled={m.is_downloading}
                  onClick={() => downloadWhisper(m.id, m.label, m.size)}
                >
                  {m.is_downloading ? "Downloading…" : "Download"}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* ── Ollama (AI / skills) ── */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginTop: "2rem" }}>
        <h1 style={{ margin: 0 }}>AI model</h1>
        <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
          <span className="subtle" style={{ fontSize: "0.8rem" }}>AI cleanup</span>
          <button
            type="button"
            className={`toggle ${aiOn ? "on" : "off"}`}
            onClick={toggleAiCleanup}
            aria-pressed={aiOn}
          >
            <span className="toggle-dot" />
          </button>
        </div>
      </div>
      <p className="subtle">
        Used for AI cleanup and skills. Pull models with{" "}
        <code>ollama pull &lt;model&gt;</code>, then click to activate.
      </p>

      <div className="pane-section">
        <ul className="row-list">
          {ollamaModels.map((m) => (
            <li
              key={m.tag}
              className={`row ${m.is_pulled ? "selectable" : ""} ${m.is_active ? "active" : ""}`}
              onClick={() => m.is_pulled && !m.is_active && activateOllama(m.tag)}
            >
              <div className="row-title">
                {m.is_active ? "✓ " : ""}
                {m.display_name}{" "}
                <span className="subtle">
                  ({m.author} · {m.size})
                </span>
              </div>
              {m.is_active ? (
                <span className="badge">active</span>
              ) : m.is_pulled ? (
                <span className="row-hint subtle">Click to activate</span>
              ) : (
                <span className="row-hint subtle">
                  <code>ollama pull {m.tag}</code>
                </span>
              )}
            </li>
          ))}
        </ul>
      </div>

      {err && <div className="pane-error">{err}</div>}
    </>
  );
}
