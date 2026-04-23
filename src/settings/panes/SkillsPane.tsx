import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Skill = {
  name: string;
  description: string;
  triggers: string[];
  source: "builtin" | "user";
  native: string | null;
  has_builtin_default: boolean;
};

type Mode =
  | { kind: "list" }
  | { kind: "edit"; name: string }
  | { kind: "create" };

const NEW_TEMPLATE = `---
name: my-skill
description: One-line description shown in Settings
---

## Triggers
- my skill {body...}

## System Prompt
Restructure the following:

{{body}}

Return only the polished text.

## Output Template
{{llm_output}}
`;

export function SkillsPane() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [mode, setMode] = useState<Mode>({ kind: "list" });
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

  if (mode.kind === "edit") {
    return (
      <SkillEditor
        name={mode.name}
        skill={skills.find((s) => s.name === mode.name) || null}
        onClose={(changed) => {
          setMode({ kind: "list" });
          if (changed) refresh();
        }}
      />
    );
  }
  if (mode.kind === "create") {
    return (
      <SkillCreator
        onClose={(changed) => {
          setMode({ kind: "list" });
          if (changed) refresh();
        }}
      />
    );
  }

  return (
    <>
      <h1>Skills</h1>
      <p className="subtle">
        Skills are markdown files. When a dictation starts with one of a
        skill's trigger phrases, that skill handles the whole transformation.
        Speech that doesn't match any skill follows the default cleanup.
      </p>

      <div className="pane-section">
        <div className="section-header">
          <h2>Active skills</h2>
          <button
            type="button"
            className="secondary"
            onClick={() => setMode({ kind: "create" })}
          >
            + New skill
          </button>
        </div>

        {skills.length === 0 ? (
          <div className="empty-hint">No skills loaded.</div>
        ) : (
          <ul className="row-list">
            {skills.map((s) => {
              const open = expanded === s.name;
              return (
                <li key={s.name} className="row column">
                  <div className="row-clickable" onClick={() => setExpanded(open ? null : s.name)}>
                    <div className="row-main">
                      <div className="row-title">
                        {s.name}{" "}
                        {s.source === "user" && s.has_builtin_default && (
                          <span className="subtle">(customized)</span>
                        )}
                      </div>
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
                      <div className="detail-actions">
                        <button
                          type="button"
                          className="primary"
                          onClick={() => setMode({ kind: "edit", name: s.name })}
                        >
                          Edit markdown
                        </button>
                      </div>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div className="pane-section">
        <h2>How skills work</h2>
        <p className="subtle">
          Every skill is a markdown file with three sections: frontmatter
          (name, description), <code>## Triggers</code> (bulleted phrases),
          and <code>## System Prompt</code> (sent to Ollama). Optional{" "}
          <code>## Output Template</code> wraps the response.
        </p>
        <p className="hint-callout">
          Placeholders in triggers: <span className="ph">{"{name}"}</span>{" "}
          captures a single word,{" "}
          <span className="ph">{"{body...}"}</span> captures the rest.
          Same placeholders are available in the prompt and template via{" "}
          <code>{"{{name}}"}</code>, plus built-ins{" "}
          <code>{"{{user_name}}"}</code>, <code>{"{{sign_off}}"}</code>, and{" "}
          <code>{"{{llm_output}}"}</code>.
        </p>
      </div>

      {err && <div className="pane-error">{err}</div>}
    </>
  );
}

// ── editor for an existing skill ──────────────────────────────────────────

function SkillEditor({
  name,
  skill,
  onClose,
}: {
  name: string;
  skill: Skill | null;
  onClose: (changed: boolean) => void;
}) {
  const [draft, setDraft] = useState<string>("");
  const [initial, setInitial] = useState<string>("");
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const src = await invoke<string>("skill_get_source", { name });
        setDraft(src);
        setInitial(src);
      } catch (e) {
        setErr(String(e));
      }
    })();
  }, [name]);

  const dirty = draft !== initial;

  const save = async () => {
    setSaving(true);
    setErr(null);
    try {
      await invoke("skill_save", { name, markdown: draft });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  const resetToDefault = async () => {
    if (!skill?.has_builtin_default) return;
    if (
      !window.confirm(
        `Reset "${name}" to its factory default? Your customizations will be removed.`,
      )
    )
      return;
    try {
      await invoke("skill_reset", { name });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    }
  };

  const deleteSkill = async () => {
    if (!window.confirm(`Delete "${name}"? This cannot be undone.`)) return;
    try {
      await invoke("skill_reset", { name });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    }
  };

  const reloadFromDefault = async () => {
    try {
      const src = await invoke<string | null>("skill_get_default_source", {
        name,
      });
      if (src) setDraft(src);
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <>
      <div className="editor-header">
        <button type="button" className="back-btn" onClick={() => onClose(false)}>
          ← Back to skills
        </button>
        <h1>Edit {name}</h1>
        {skill && (
          <span className={`badge ${skill.source}`}>{skill.source}</span>
        )}
      </div>

      <textarea
        className="skill-editor"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        spellCheck={false}
        autoFocus
      />

      {err && <div className="pane-error">{err}</div>}

      <div className="editor-actions">
        <button
          type="button"
          className="primary"
          disabled={!dirty || saving}
          onClick={save}
        >
          {saving ? "Saving…" : "Save"}
        </button>
        <button
          type="button"
          className="secondary"
          onClick={() => onClose(false)}
        >
          Cancel
        </button>
        <div className="spacer" />
        {skill?.has_builtin_default && skill.source === "user" && (
          <button
            type="button"
            className="secondary"
            onClick={resetToDefault}
            title="Remove your customizations and restore the factory version"
          >
            Reset to default
          </button>
        )}
        {skill?.has_builtin_default && (
          <button
            type="button"
            className="secondary"
            onClick={reloadFromDefault}
            title="Load the factory markdown into the editor (does not save)"
          >
            Load default into editor
          </button>
        )}
        {skill &&
          skill.source === "user" &&
          !skill.has_builtin_default && (
            <button type="button" className="danger-btn" onClick={deleteSkill}>
              Delete skill
            </button>
          )}
      </div>
    </>
  );
}

// ── new-skill form ─────────────────────────────────────────────────────────

function SkillCreator({ onClose }: { onClose: (changed: boolean) => void }) {
  const [draft, setDraft] = useState<string>(NEW_TEMPLATE);
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    setErr(null);
    try {
      await invoke<string>("skill_create", { markdown: draft });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <div className="editor-header">
        <button type="button" className="back-btn" onClick={() => onClose(false)}>
          ← Back to skills
        </button>
        <h1>New skill</h1>
      </div>

      <p className="subtle">
        Edit the template below, then click Create. The name in the
        frontmatter becomes the skill's id; change it before saving.
      </p>

      <textarea
        className="skill-editor"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        spellCheck={false}
        autoFocus
      />

      {err && <div className="pane-error">{err}</div>}

      <div className="editor-actions">
        <button
          type="button"
          className="primary"
          disabled={saving}
          onClick={save}
        >
          {saving ? "Saving…" : "Create"}
        </button>
        <button
          type="button"
          className="secondary"
          onClick={() => onClose(false)}
        >
          Cancel
        </button>
      </div>
    </>
  );
}

// ── helpers ────────────────────────────────────────────────────────────────

function prettyTrigger(t: string): React.ReactNode {
  const parts: Array<{ text: string; placeholder: boolean }> = [];
  const re = /\{[^}]+\}/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(t)) !== null) {
    if (m.index > last)
      parts.push({ text: t.slice(last, m.index), placeholder: false });
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
