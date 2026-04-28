// Mini animated pill preview — reuses the same CSS classes as the real overlay.
function MiniPill({
  variant,
  animate = false,
  check = false,
  dim,
  label,
}: {
  variant: "listen" | "process" | "done";
  animate?: boolean;
  check?: boolean;
  dim?: string;
  label: string;
}) {
  const heights = [4, 8, 15, 13, 8, 4];
  const delays  = [0.30, 0.15, 0, 0.10, 0.22, 0.36];
  return (
    <div className="tip-pill">
      <div className={`lm lm--${variant}`}>
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            className={`lm-bar${animate ? " lm-bar-animate" : ""}`}
            style={{ height: heights[i], ...(animate ? { animationDelay: `${delays[i]}s` } : {}) }}
          />
        ))}
        <div className={`lm-cursor${animate ? " lm-cursor-blink" : ""}`} />
        {[3, 4, 5].map((i) => (
          <div
            key={i}
            className={`lm-bar${animate ? " lm-bar-animate" : ""}`}
            style={{ height: heights[i], ...(animate ? { animationDelay: `${delays[i]}s` } : {}) }}
          />
        ))}
      </div>
      {check && <span className="tip-pill-check">✓</span>}
      <span className="tip-pill-label">
        {dim && <span className="tip-pill-dim">{dim}</span>}
        {label}
      </span>
    </div>
  );
}

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
            <strong>release</strong> to transcribe and paste into the focused app.
          </li>
          <li>
            Quick taps under ¼ second are ignored — hold until you finish speaking.
          </li>
          <li>Everything runs locally. No audio ever leaves your device.</li>
        </ul>
      </div>

      {/* ── Status indicator ───────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Status indicator</h2>
        <p className="subtle">
          A floating pill appears at the bottom of your screen during each
          dictation. When you're idle, it's gone — nothing to look at until you
          start speaking.
        </p>

        <div className="status-legend">
          <div className="status-legend-row">
            <MiniPill variant="listen" animate label="listening..." />
            <span className="status-legend-desc">
              Microphone is live — hold the shortcut and speak now.
            </span>
          </div>
          <div className="status-legend-row">
            <MiniPill variant="process" animate label="processing…" />
            <span className="status-legend-desc">
              Transcribing and running AI cleanup on your speech.
            </span>
          </div>
          <div className="status-legend-row">
            <MiniPill variant="done" check label="done" />
            <span className="status-legend-desc">
              Text pasted successfully — clears in about a second.
            </span>
          </div>
          <div className="status-legend-row">
            <MiniPill variant="done" check dim="skill: " label="commit" />
            <span className="status-legend-desc">
              A skill or phrase fired — shows its name so you can verify.
            </span>
          </div>
        </div>

        <div className="hint-callout">
          <strong>Menu bar icon</strong> — always a static white logo. A small{" "}
          <span style={{ color: "#ef4444", fontWeight: 600 }}>red dot</span>{" "}
          badge appears in the corner while the model is loading or
          initializing. Once it disappears, Soll is ready.
        </div>
      </div>

      {/* ── Skills ─────────────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Skills — AI-powered voice macros</h2>
        <p className="subtle">
          A skill listens for a trigger phrase and reshapes your dictation into
          a specific format using a local AI model.
        </p>
        <ul className="tips-list">
          <li>
            Speak a <strong>trigger phrase</strong> to activate — e.g.{" "}
            <em>"git commit fixed the null pointer bug"</em> → a clean commit
            message.
          </li>
          <li>
            Or say <strong>skill [trigger]</strong> to invoke directly — e.g.{" "}
            <code>skill git commit fixed the null pointer bug</code>.
          </li>
          <li>
            Good candidates for your own skills: <em>Slack messages</em>,{" "}
            <em>meeting notes</em>, <em>todo items</em>, <em>commit messages</em>{" "}
            — anywhere the output has a predictable shape.
          </li>
        </ul>
      </div>

      {/* ── Phrases ────────────────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Phrases — instant text snippets</h2>
        <p className="subtle">
          A phrase pastes a saved block of text verbatim — no AI, no latency.
          Great for Calendly links, signatures, canned replies.
        </p>
        <ul className="tips-list">
          <li>
            Speak a trigger phrase to paste instantly — e.g.{" "}
            <em>"calendly"</em> → your booking link.
          </li>
          <li>
            Or say <strong>phrase [trigger]</strong> to invoke directly — e.g.{" "}
            <code>phrase calendly</code>.
          </li>
          <li>
            Use <code>[body]</code> or <code>&lt;variable&gt;</code> in the
            phrase text to splice in words captured from the trigger.
          </li>
        </ul>
      </div>

      {/* ── Self-corrections ───────────────────────────────────────── */}
      <div className="pane-section tips-section">
        <h2>Self-correct mid-sentence</h2>
        <p className="subtle">
          Realized you said the wrong thing? Just say the right one — Soll
          rewrites it. Works for numbers, times, weekdays, and short names.
        </p>
        <ul className="tips-list">
          <li>
            <em>"meet at 5 pm actually 6 pm"</em> → <code>meet at 6 pm</code>
          </li>
          <li>
            <em>"due Tuesday no wait Wednesday"</em> → <code>due Wednesday</code>
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

      {/* ── Quiet cleanup ──────────────────────────────────────────── */}
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
