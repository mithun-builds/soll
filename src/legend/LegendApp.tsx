type Row = {
  key: string;
  title: string;
  swatch: string;
  pulse: "none" | "slow" | "medium";
  when: string;
  action: string;
};

// 4-color palette. Loading / Initializing / Processing all share the blue
// "Working" state — the user only needs to distinguish WAIT from SPEAK.
// The tray menu's first line still shows the specific sub-state in text.
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

export function LegendApp() {
  return (
    <div className="legend">
      <header className="legend-header">
        <h1>Status Legend</h1>
        <p className="subtle">
          Four colors, one rule: watch the tray icon for when to speak. The
          menu's top line always shows exact status text.
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
          Prefix your speech with one of these phrases to format as a list
          automatically. No trigger = plain paragraph.
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
          Examples:
        </p>
        <ul className="example-list">
          <li>
            <em>"ordinal list 1 apple 2 banana 3 milk"</em>
          </li>
          <li>
            <em>"bullet list milk, bread and eggs"</em>
          </li>
        </ul>
      </section>

      <footer className="legend-footer subtle">
        Hold ⌃⇧Space anywhere to dictate into the focused app.
      </footer>
    </div>
  );
}
