import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Skill = {
  name: string;
  description: string;
  triggers: string[];
  source: "builtin" | "user";
  native: string | null;
};

export function SkillsPane() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<Skill[]>("skill_list");
      setSkills(list);
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <>
      <h1>Skills</h1>
      <p className="subtle">
        Skills are markdown files. When a dictation starts with one of a
        skill's trigger phrases, that skill handles the whole transformation.
        Speech that doesn't match any skill follows the default cleanup.
      </p>

      <div className="pane-section">
        <h2>Active skills</h2>
        {skills.length === 0 ? (
          <div className="empty-hint">No skills loaded.</div>
        ) : (
          <ul className="row-list">
            {skills.map((s) => {
              const open = expanded === s.name;
              return (
                <li key={s.name} className="row column">
                  <div
                    className="row-clickable"
                    onClick={() => setExpanded(open ? null : s.name)}
                  >
                    <div className="row-main">
                      <div className="row-title">{s.name}</div>
                      <div className="subtle">{s.description}</div>
                    </div>
                    <span className={`badge ${s.source}`}>{s.source}</span>
                  </div>
                  {open && (
                    <div className="row-details">
                      <div className="detail-row">
                        <span className="subtle">Say one of</span>
                        <ul className="trigger-list">
                          {s.triggers.map((t, i) => (
                            <li key={i}>
                              <code>{prettyTrigger(t)}</code>
                            </li>
                          ))}
                        </ul>
                      </div>
                      {s.native && (
                        <div className="detail-row">
                          <span className="subtle">Native hook</span>
                          <code>{s.native}</code>
                        </div>
                      )}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div className="pane-section">
        <h2>Add your own</h2>
        <p className="subtle">
          Drop a <code>.md</code> file into the skills directory, then click
          Reload. The file's <code>## Triggers</code> section lists plain-English
          phrases users can say; <code>{"{name}"}</code> captures a single word,{" "}
          <code>{"{name...}"}</code> captures the rest.
        </p>
        <p className="hint-callout">
          Skills directory:{" "}
          <code>~/Library/Application Support/com.svara.app/skills/</code>
        </p>
        <button
          type="button"
          className="secondary"
          onClick={refresh}
          title="Re-scan the skills directory"
        >
          Reload skills
        </button>
      </div>

      {err && <div className="pane-error">{err}</div>}
    </>
  );
}

// Wrap placeholder tokens in visually distinct spans so "{body...}" reads
// like a parameter rather than a typo.
function prettyTrigger(t: string): React.ReactNode {
  // Simple split on {…} so we can style placeholders differently.
  const parts: Array<{ text: string; placeholder: boolean }> = [];
  const re = /\{[^}]+\}/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(t)) !== null) {
    if (m.index > last) parts.push({ text: t.slice(last, m.index), placeholder: false });
    parts.push({ text: m[0], placeholder: true });
    last = m.index + m[0].length;
  }
  if (last < t.length) parts.push({ text: t.slice(last), placeholder: false });
  return (
    <>
      {parts.map((p, i) =>
        p.placeholder ? (
          <span key={i} className="ph">
            {p.text}
          </span>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}
