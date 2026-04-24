type StatusRow = {
  key: string;
  title: string;
  swatch: string;
  pulse: "none" | "slow" | "medium";
  when: string;
  action: string;
};

const STATUS_ROWS: StatusRow[] = [
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

export function TipsPane() {
  return (
    <>
      <h1>Tips & Tricks</h1>
      <p className="subtle">
        Everything worth knowing about dictating with Soll — in one place.
      </p>

      {/* ── Push to talk ───────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Push to talk</h2>
        <ul className="tips-list">
          <li>
            <strong>Hold</strong> <code>⌃⇧Space</code> anywhere to record;{" "}
            <strong>release</strong> to transcribe and paste into the focused
            app.
          </li>
          <li>
            Quick taps under ¼ second are ignored — hold until you finish
            speaking.
          </li>
          <li>Everything runs locally. No audio ever leaves your Mac.</li>
        </ul>
      </div>

      {/* ── Status colors ──────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Status indicator</h2>
        <p className="subtle">
          Watch the tray icon to know when to speak. The menu's top line always
          shows the exact status text.
        </p>
        <div className="legend-list">
          {STATUS_ROWS.map((row) => (
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
      </div>

      {/* ── List shortcuts ─────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Format a list while dictating</h2>
        <p className="subtle">
          Start your sentence with one of these phrases and Soll formats the
          rest as a list. No AI cleanup runs on lists — what you say is what
          you get.
        </p>
        <ul className="tips-list">
          <li>
            <code>bullet list …</code> or <code>bullets …</code> — bulleted list
          </li>
          <li>
            <code>numbered list …</code> or <code>ordinal list …</code> —
            numbered list
          </li>
        </ul>
        <p className="hint-callout">
          <em>"ordinal list apples, bananas, milk"</em> →{" "}
          <code>1. Apples · 2. Bananas · 3. Milk</code>
        </p>
      </div>

      {/* ── Self-corrections ───────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Self-correct mid-sentence</h2>
        <p className="subtle">
          Realized you said the wrong thing? Just say the right one and Soll
          rewrites it. Works for numbers, times, weekdays, and short names.
        </p>
        <ul className="tips-list">
          <li>
            <em>"meet at 5 pm actually 6 pm"</em> → <code>meet at 6 pm</code>
          </li>
          <li>
            <em>"due Tuesday no wait Wednesday"</em> →{" "}
            <code>due Wednesday</code>
          </li>
          <li>
            <em>"3 apples I mean 4 apples"</em> → <code>4 apples</code>
          </li>
        </ul>
        <p className="subtle">
          Trigger words:{" "}
          <code>actually</code>, <code>I mean</code>, <code>no wait</code>,{" "}
          <code>scratch that</code>, <code>correction</code>,{" "}
          <code>or rather</code>, <code>make that</code>, <code>sorry</code>.
        </p>
      </div>

      {/* ── Skills ─────────────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Skills turn speech into structured text</h2>
        <p className="subtle">
          A skill is a voice macro — a phrase Soll recognizes and reshapes
          into a specific format. One ships with the app; you can edit, turn
          off, delete, or add your own in the Skills pane.
        </p>
        <ul className="tips-list">
          <li>
            <em>"email Jane we'll push the launch to next Friday"</em> → a
            full, polite email signed with your name
          </li>
        </ul>
        <p className="subtle">
          Good candidates for your own skills: <em>slack messages</em>,{" "}
          <em>meeting notes</em>, <em>todo items</em>, <em>commit messages</em>{" "}
          — anywhere the output has a predictable shape.
        </p>
      </div>

      {/* ── Dictionary ─────────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Teach Soll your vocabulary</h2>
        <p className="subtle">
          Whisper mishears unusual words — brand names, jargon, acronyms,
          teammates' names. Add them to the Dictionary pane.
        </p>
        <ul className="tips-list">
          <li>Added terms bias the transcription toward the correct spelling.</li>
          <li>
            They're also preserved after AI cleanup — no more{" "}
            <em>"soll"</em> turning into <em>"sol"</em>.
          </li>
        </ul>
      </div>

      {/* ── Under the hood cleanup ─────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Stuff Soll quietly cleans up</h2>
        <ul className="tips-list">
          <li>
            Filler words (<em>"um"</em>, <em>"uh"</em>, <em>"you know"</em>)
            removed by AI cleanup.
          </li>
          <li>
            Shouty <code>ALL-CAPS</code> words get lowercased —{" "}
            <em>"tomorrow"</em>, not <em>"TOMORROW"</em>. Known acronyms like{" "}
            <code>API</code>, <code>URL</code>, <code>CEO</code>,{" "}
            <code>ASAP</code> are kept.
          </li>
          <li>
            LLM preambles like <em>"Here's the polished version:"</em> are
            stripped before paste.
          </li>
          <li>
            <em>"I"</em>, weekdays, and months get capitalized in emails and
            other polished output.
          </li>
        </ul>
      </div>

      {/* ── Your name ──────────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Set your name once</h2>
        <p className="subtle">
          Drop your name into General and skills like <code>email</code> sign
          off with it automatically. Inside any custom skill, reference it as{" "}
          <code>[name]</code>.
        </p>
      </div>
    </>
  );
}
