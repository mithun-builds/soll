import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Model = {
  id: string;
  label: string;
  size: string;
  is_cached: boolean;
  is_active: boolean;
  is_downloading: boolean;
};

export function ModelsPane() {
  const [models, setModels] = useState<Model[]>([]);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<Model[]>("models_list");
      setModels(list);
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    // Poll while a download is in flight so progress label + cache state
    // update without the user needing to click around.
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  const activate = async (id: string) => {
    try {
      await invoke("model_activate", { id });
      refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const download = async (id: string, label: string, size: string) => {
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

  const cached = models.filter((m) => m.is_cached);
  const uncached = models.filter((m) => !m.is_cached);

  return (
    <>
      <h1>Whisper model</h1>
      <p className="subtle">
        Bigger models are more accurate but slower. Click any cached model
        to activate it.
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
                onClick={() => !m.is_active && activate(m.id)}
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
                  onClick={() => download(m.id, m.label, m.size)}
                >
                  {m.is_downloading ? "Downloading…" : "Download"}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {err && <div className="pane-error">{err}</div>}
    </>
  );
}
