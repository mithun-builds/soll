type Row = {
  key: string;
  title: string;
  swatch: string;
  pulse: "none" | "slow" | "medium";
  when: string;
  action: string;
};

const ROWS: Row[] = [
  {
    key: "idle",
    title: "Idle",
    swatch: "#fde047",
    pulse: "none",
    when: "Ready for the next dictation",
    action: "Hold ⌃⇧Space when you want to speak",
  },
  {
    key: "working",
    title: "Working",
    swatch: "#38bdf8",
    pulse: "medium",
    when: "Loading, initializing mic, or processing",
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
    key: "transcribed",
    title: "Transcribed",
    swatch: "#22c55e",
    pulse: "none",
    when: "Text just pasted into the focused app",
    action: "Done — returns to Idle in ~1 second",
  },
];

export function LegendPane() {
  return (
    <>
      <h1>Status Legend</h1>
      <p className="subtle">
        Four colors, one rule: watch the tray icon for when to speak. The
        menu's top line always shows exact status text.
      </p>

      <div className="pane-section legend-list">
        {ROWS.map((row) => (
          <div key={row.key} className="legend-row">
            <div
              className={`legend-dot legend-dot-${row.pulse}`}
              style={{ background: row.swatch }}
            />
            <div>
              <div className="field-label">{row.title}</div>
              <div className="subtle">{row.when}</div>
              <div>{row.action}</div>
            </div>
          </div>
        ))}
      </div>

      <div className="pane-section">
        <h2>List triggers</h2>
        <p className="subtle">
          Prefix speech with one of these phrases to format as a list:
        </p>
        <ul className="plain-list">
          <li>
            <code>bullet list …</code> — bullets
          </li>
          <li>
            <code>ordinal list …</code> or <code>numbered list …</code> —
            numbered
          </li>
        </ul>
        <p className="hint-callout">
          Example: <em>"ordinal list 1 apple 2 banana 3 milk"</em> →{" "}
          <code>1. Apple · 2. Banana · 3. Milk</code>
        </p>
      </div>
    </>
  );
}
