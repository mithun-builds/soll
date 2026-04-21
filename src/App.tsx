import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type PillState =
  | { kind: "idle" }
  | { kind: "listening" }
  | { kind: "processing" }
  | { kind: "done"; text: string }
  | { kind: "error"; message: string };

export function App() {
  const [state, setState] = useState<PillState>({ kind: "idle" });

  useEffect(() => {
    const unlisten = listen<PillState>("svara:state", (event) => {
      setState(event.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <div className={`pill pill-${state.kind}`}>
      <div className="dot" />
      <div className="label">{labelFor(state)}</div>
    </div>
  );
}

function labelFor(s: PillState): string {
  switch (s.kind) {
    case "idle":
      return "Hold ⌃⇧Space";
    case "listening":
      return "Listening…";
    case "processing":
      return "Polishing…";
    case "done":
      return "✓ Pasted";
    case "error":
      return `× ${s.message}`;
  }
}
