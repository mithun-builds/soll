import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type OverlayState =
  | { kind: "recording" }
  | { kind: "processing" }
  | { kind: "skill_done"; name: string; is_phrase: boolean }
  | { kind: "transcribed" }
  | { kind: "idle" };

export function OverlayApp() {
  const [st, setSt] = useState<OverlayState | null>(null);

  useEffect(() => {
    let cancel: (() => void) | null = null;
    listen<OverlayState>("overlay-update", (e) => {
      setSt(e.payload);
    }).then((fn) => {
      cancel = fn;
    }).catch(() => {});
    return () => { cancel?.(); };
  }, []);

  if (!st || st.kind === "idle") return null;

  return (
    <div className="overlay-pill">
      {st.kind === "recording"   && <Recording />}
      {st.kind === "processing"  && <Processing />}
      {st.kind === "skill_done"  && <SkillDone name={st.name} isPhrase={st.is_phrase} />}
      {st.kind === "transcribed" && <Transcribed />}
    </div>
  );
}

// ── logo mark ──────────────────────────────────────────────────────────────
//
// Inline SVG recreation of the wave + I-beam cursor brand mark.
// waveAnimate  — bars pulse up/down (recording state)
// cursorBlink  — amber cursor blinks (recording + processing)

// variant "listen"  → white bars + yellow cursor (recording)
// variant "process" → yellow bars + white cursor (processing)
// variant "done"    → white bars + yellow cursor, static
function LogoMark({
  waveAnimate = false,
  cursorBlink = false,
  variant = "listen",
}: {
  waveAnimate?: boolean;
  cursorBlink?: boolean;
  variant?: "listen" | "process" | "done";
}) {
  // [height, animDelay] for each bar, left-to-right around the cursor
  const leftBars:  [number, number][] = [[4, 0.30], [8, 0.15], [15, 0.00]];
  const rightBars: [number, number][] = [[13, 0.10], [8, 0.22], [4, 0.36]];

  return (
    <div className={`lm lm--${variant}`}>
      {leftBars.map(([h, delay], i) => (
        <div
          key={i}
          className={`lm-bar${waveAnimate ? " lm-bar-animate" : ""}`}
          style={{ height: h, ...(waveAnimate ? { animationDelay: `${delay}s` } : {}) }}
        />
      ))}
      <div className={`lm-cursor${cursorBlink ? " lm-cursor-blink" : ""}`} />
      {rightBars.map(([h, delay], i) => (
        <div
          key={i}
          className={`lm-bar${waveAnimate ? " lm-bar-animate" : ""}`}
          style={{ height: h, ...(waveAnimate ? { animationDelay: `${delay}s` } : {}) }}
        />
      ))}
    </div>
  );
}

// ── states ─────────────────────────────────────────────────────────────────

function Recording() {
  return (
    <div className="overlay-row">
      <LogoMark waveAnimate cursorBlink />
      <span className="overlay-label">listening</span>
    </div>
  );
}

function Processing() {
  return (
    <div className="overlay-row">
      <LogoMark waveAnimate cursorBlink variant="process" />
      <span className="overlay-label subtle">processing…</span>
    </div>
  );
}

function SkillDone({ name, isPhrase }: { name: string; isPhrase: boolean }) {
  return (
    <div className="overlay-row overlay-success">
      <LogoMark />
      <span className="overlay-check">✓</span>
      <span className="overlay-label">
        <span className="overlay-dim">{isPhrase ? "phrase: " : "skill: "}</span>
        {name}
      </span>
    </div>
  );
}

function Transcribed() {
  return (
    <div className="overlay-row overlay-success">
      <LogoMark />
      <span className="overlay-check">✓</span>
      <span className="overlay-label">done</span>
    </div>
  );
}
