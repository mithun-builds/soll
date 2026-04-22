type Row = {
  key: string;
  title: string;
  swatch: string; // CSS color for the dot
  pulse: "none" | "slow" | "medium" | "fast";
  when: string; // user-facing description
  action: string; // what the user should do
};

// The canonical source of truth for tray state colors mirrors
// src-tauri/icons/tray_*.png + src-tauri/src/tray.rs::TrayState.
const ROWS: Row[] = [
  {
    key: "loading",
    title: "Loading",
    swatch: "#94a3b8",
    pulse: "slow",
    when: "App is starting up",
    action: "Wait — whisper model + AI cleanup are warming up",
  },
  {
    key: "idle",
    title: "Idle",
    swatch: "#fde047",
    pulse: "none",
    when: "Ready for the next dictation",
    action: "Hold ⌃⇧Space when you want to speak",
  },
  {
    key: "initializing",
    title: "Initializing",
    swatch: "#38bdf8",
    pulse: "fast",
    when: "Hotkey pressed; microphone warming up",
    action: "Wait — don't speak yet",
  },
  {
    key: "transcribing",
    title: "Transcribing",
    swatch: "#ef4444",
    pulse: "medium",
    when: "Microphone is live and capturing your voice",
    action: "Speak now",
  },
  {
    key: "processing",
    title: "Processing",
    swatch: "#f59e0b",
    pulse: "medium",
    when: "Whisper + Ollama running on your speech",
    action: "Wait — text will paste automatically",
  },
  {
    key: "transcribed",
    title: "Transcribed",
    swatch: "#22c55e",
    pulse: "none",
    when: "Text just pasted into the focused app",
    action: "Done — state returns to Idle in ~1 second",
  },
];

export function LegendApp() {
  return (
    <div className="legend">
      <header className="legend-header">
        <h1>Status Legend</h1>
        <p className="subtle">
          The tray-icon color tells you what Svara is doing. Same color
          appears in the menu status line and tooltip.
        </p>
      </header>

      <div className="legend-list">
        {ROWS.map((row) => (
          <div key={row.key} className="legend-row">
            <div
              className={`legend-dot legend-dot-${row.pulse}`}
              style={{ background: row.swatch }}
            />
            <div className="legend-copy">
              <div className="legend-title">{row.title}</div>
              <div className="legend-when">{row.when}</div>
              <div className="legend-action">{row.action}</div>
            </div>
          </div>
        ))}
      </div>

      <section className="legend-triggers">
        <h2>List triggers</h2>
        <p className="subtle">
          Say one of these phrases before your items to auto-format as a list.
          No trigger = plain paragraph.
        </p>
        <ul>
          <li>
            <code>bullet list …</code>
            <span> — bullets (- item)</span>
          </li>
          <li>
            <code>ordinal list …</code> <span className="muted">or</span>{" "}
            <code>numbered list …</code>
            <span> — numbered (1. 2. 3.)</span>
          </li>
        </ul>
        <p className="subtle example">
          Example: <em>"ordinal list coffee, tea, water"</em> →{" "}
          <code>1. Coffee, 2. Tea, 3. Water</code>
        </p>
      </section>

      <footer className="legend-footer subtle">
        Hold ⌃⇧Space anywhere to dictate into the focused app.
      </footer>
    </div>
  );
}
