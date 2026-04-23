import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Skill = {
  name: string;
  description: string;
  trigger: string;
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
        Skills are markdown files that detect a trigger phrase and transform
        your dictation. The first matching skill wins; unmatched speech
        follows the default pipeline.
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
                        <span className="subtle">Trigger</span>
                        <code className="trigger">{s.trigger}</code>
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
          Reload. See the markdown format in{" "}
          <code>src-tauri/skills/email.md</code> in the repo.
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
