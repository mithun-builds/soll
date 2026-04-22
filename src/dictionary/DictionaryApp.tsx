import { useEffect, useState, useCallback, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

type DictEntry = {
  word: string;
  weight: number;
  added_at: string;
};

export function DictionaryApp() {
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
    <div className="dict">
      <header className="dict-header">
        <h1>Dictionary</h1>
        <p className="subtle">
          Names, jargon, and specific terms Svara should always get right.
        </p>
        <p className="dict-hint">
          Type each term in the <strong>exact casing</strong> you want
          pasted. <code>GrowthBook</code> and <code>Growthbook</code> are
          different entries — whatever you type here is what appears in
          your text.
        </p>
      </header>

      <form className="dict-add" onSubmit={onAdd}>
        <input
          autoFocus
          type="text"
          placeholder="Add a word (e.g. Homelane, Vrishti, GrowthBook)"
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <button type="submit" disabled={!input.trim()}>
          Add
        </button>
      </form>

      {error && <div className="dict-error">{error}</div>}

      {loading ? (
        <div className="dict-loading">Loading…</div>
      ) : entries.length === 0 ? (
        <div className="dict-empty">
          No words yet. Add your first term above — it will be injected into
          Whisper's decoding prompt and preserved through AI cleanup.
        </div>
      ) : (
        <ul className="dict-list">
          {entries.map((e) => (
            <li key={e.word}>
              <span className="dict-word">{e.word}</span>
              <span className="dict-meta">weight {e.weight}</span>
              <button
                className="dict-remove"
                onClick={() => onRemove(e.word)}
                title="Remove"
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}

      <footer className="dict-footer">
        <div className="subtle">
          {entries.length} {entries.length === 1 ? "entry" : "entries"}
        </div>
      </footer>
    </div>
  );
}
