import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type OverlayState =
  | { kind: "recording" }
  | { kind: "processing" }
  | { kind: "skill_done"; name: string }
  | { kind: "transcribed" };

export function OverlayApp() {
  const [st, setSt] = useState<OverlayState | null>(null);

  useEffect(() => {
    let cancel: (() => void) | null = null;
    listen<OverlayState>("overlay-update", (e) => {
      setSt(e.payload);
    }).then((fn) => {
      cancel = fn;
    });
    return () => { cancel?.(); };
  }, []);

  if (!st) return null;

  return (
    <div className="overlay-pill">
      {st.kind === "recording"   && <Recording />}
      {st.kind === "processing"  && <Processing />}
      {st.kind === "skill_done"  && <SkillDone name={st.name} />}
      {st.kind === "transcribed" && <Transcribed />}
    </div>
  );
}

// ── states ─────────────────────────────────────────────────────────────────

function Recording() {
  return (
    <div className="overlay-row">
      <div className="wave">
        {[0, 1, 2, 3, 4].map((i) => (
          <div
            key={i}
            className="wave-bar"
            style={{ animationDelay: `${i * 0.12}s` }}
          />
        ))}
      </div>
      <span className="overlay-label">Listening</span>
    </div>
  );
}

function Processing() {
  return (
    <div className="overlay-row">
      <div className="dots">
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            className="dot"
            style={{ animationDelay: `${i * 0.22}s` }}
          />
        ))}
      </div>
      <span className="overlay-label subtle">Processing…</span>
    </div>
  );
}

function SkillDone({ name }: { name: string }) {
  return (
    <div className="overlay-row overlay-success">
      <span className="overlay-check">✓</span>
      <span className="overlay-label">
        <span className="overlay-dim">skill: </span>
        {name}
      </span>
    </div>
  );
}

function Transcribed() {
  return (
    <div className="overlay-row overlay-success">
      <span className="overlay-check">✓</span>
      <span className="overlay-label">Done</span>
    </div>
  );
}
