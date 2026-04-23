import { useEffect, useState, useCallback, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

type DictEntry = {
  word: string;
  weight: number;
  added_at: string;
};

export function DictionaryPane() {
  const [entries, setEntries] = useState<DictEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const rows = await invoke<DictEntry[]>("dict_list");
      setEntries(rows);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const onAdd = async (e: FormEvent) => {
    e.preventDefault();
    const word = input.trim();
    if (!word) return;
    try {
      const rows = await invoke<DictEntry[]>("dict_add", { word });
      setEntries(rows);
      setInput("");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const onRemove = async (word: string) => {
    try {
      const rows = await invoke<DictEntry[]>("dict_remove", { word });
      setEntries(rows);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <h1>Dictionary</h1>
      <p className="subtle">
        Names, jargon, and specific terms Svara should always spell correctly.
      </p>
      <p className="hint-callout">
        Type each term in the <strong>exact casing</strong> you want pasted.
        <code>GrowthBook</code> and <code>Growthbook</code> are different
        entries — whatever you type here is what appears in your text.
      </p>

      <form className="pane-section dict-add" onSubmit={onAdd}>
        <input
          autoFocus
          type="text"
          placeholder="Add a word (e.g. HomeLane, Vrishti, GrowthBook)"
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <button type="submit" className="primary" disabled={!input.trim()}>
          Add
        </button>
      </form>

      {error && <div className="pane-error">{error}</div>}

      {loading ? (
        <div className="pane-loading">Loading…</div>
      ) : entries.length === 0 ? (
        <div className="empty-hint">
          No words yet. Add your first term above.
        </div>
      ) : (
        <ul className="row-list">
          {entries.map((e) => (
            <li key={e.word} className="row">
              <span className="row-title">{e.word}</span>
              <span className="subtle">weight {e.weight}</span>
              <button
                className="icon-button danger"
                onClick={() => onRemove(e.word)}
                title="Remove"
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="subtle hint">
        {entries.length} {entries.length === 1 ? "entry" : "entries"}
      </div>
    </>
  );
}
