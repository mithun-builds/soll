import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";

// ── types ─────────────────────────────────────────────────────────────────

type SkillKind = "ai" | "phrase";

type Skill = {
  name: string;
  description: string;
  triggers: string[];
  native: string | null;
  enabled: boolean;
  kind: SkillKind;
};

type Mode =
  | { kind: "list" }
  | { kind: "edit"; name: string }
  | { kind: "create" };

// Per-kind copy — everything user-visible lives here so the two panes
// stay in visual step with each other.
const COPY = {
  ai: {
    title: "Skills",
    subtitle:
      "Voice shortcuts that run through a local AI to transform your dictation. Click one to edit.",
    emptyHint: (
      <>
        No skills yet. Click <strong>+ New skill</strong> to create one.
      </>
    ),
    newBtn: "+ New skill",
    editorNewTitle: "New skill",
    bodyFieldLabel: "What the AI should do",
    bodyFieldHint: (
      <>
        Use <code>[body]</code> for the utterance, <code>[name]</code> for the
        user, or any <code>[variable]</code> from a trigger.
      </>
    ),
    bodyPlaceholder:
      "Rewrite the following as a short, casual Slack message.\nOutput only the message, nothing else.\n\n[body]",
    bodyRequiredError: "Instructions are required.",
    deleteConfirm: (name: string) => `Delete "${name}"?`,
  },
  phrase: {
    title: "Phrases",
    subtitle:
      "Voice shortcuts that paste a saved block of text verbatim — no AI, no latency. Click one to edit.",
    emptyHint: (
      <>
        No phrases yet. Click <strong>+ New phrase</strong> to create one.
      </>
    ),
    newBtn: "+ New phrase",
    editorNewTitle: "New phrase",
    bodyFieldLabel: "Text to paste",
    bodyFieldHint: (
      <>
        Pasted verbatim when the trigger fires. Use <code>[body]</code> or any{" "}
        <code>[variable]</code> from a trigger to splice captured words in.
      </>
    ),
    bodyPlaceholder:
      "Here's my Calendly link to book a time:\nhttps://calendly.com/you",
    bodyRequiredError: "Phrase text is required.",
    deleteConfirm: (name: string) => `Delete "${name}"?`,
  },
} as const;

// ── section data model ────────────────────────────────────────────────────
//
// A single editor form backs both kinds. The body field name (`instructions`
// vs `phrase`) is tracked by `kind`, and the markdown assembler writes the
// matching section heading.

type Sections = {
  name: string;
  description: string;
  triggers: string;
  kind: SkillKind;
  body: string;
};

function emptySections(kind: SkillKind): Sections {
  return { name: "", description: "", triggers: "", kind, body: "" };
}

function sectionsEqual(a: Sections, b: Sections): boolean {
  return (
    a.name === b.name &&
    a.description === b.description &&
    a.triggers === b.triggers &&
    a.kind === b.kind &&
    a.body === b.body
  );
}

/** Parse the raw skill markdown into editable per-section fields. Accepts
 *  both the section-based format (## Name, ## Description) and the legacy
 *  YAML-frontmatter format (---\nname:\ndescription:\n---). Strips comment
 *  lines (lines starting with `#`) and any leading `- `/`* ` bullet markers
 *  in Triggers. */
function parseSections(md: string): Sections {
  let body = md;
  let fmName = "";
  let fmDesc = "";
  const trimmed = md.trimStart();
  if (trimmed.startsWith("---")) {
    const rest = trimmed.slice(3);
    const closeIdx = rest.indexOf("\n---");
    if (closeIdx >= 0) {
      const fm = rest.slice(0, closeIdx);
      body = rest.slice(closeIdx + 4).replace(/^\n+/, "");
      for (const line of fm.split("\n")) {
        const m = line.match(/^\s*(\w+)\s*:\s*(.*?)\s*$/);
        if (!m) continue;
        if (m[1] === "name") fmName = m[2];
        if (m[1] === "description") fmDesc = m[2];
      }
    }
  }

  const rawSection = (heading: string): string => {
    const re = new RegExp(`^##\\s+${heading}\\s*$`, "m");
    const match = body.match(re);
    if (!match || match.index === undefined) return "";
    const start = match.index + match[0].length;
    const after = body.slice(start).replace(/^\n/, "");
    const nextIdx = after.search(/\n## /);
    const raw = nextIdx >= 0 ? after.slice(0, nextIdx) : after;
    return raw
      .split("\n")
      .filter((l) => !l.trimStart().startsWith("#"))
      .join("\n")
      .trim();
  };

  const firstLine = (s: string): string =>
    s.split("\n").map((l) => l.trim()).find((l) => l.length > 0) ?? "";

  const triggersRaw = rawSection("Triggers");
  const triggers = triggersRaw
    .split("\n")
    .map((l) => l.replace(/^\s*[-*]\s*/, "").trim())
    .filter((l) => l.length > 0)
    .join("\n");

  const nameSection = firstLine(rawSection("Name"));
  const descSection = firstLine(rawSection("Description"));

  // Body comes from either `## Instructions` (AI) or `## Phrase` (literal
  // paste) — the parser in Rust requires exactly one. Whichever one exists
  // tells us the kind.
  const instructions = rawSection("Instructions");
  const phrase = rawSection("Phrase");
  const kind: SkillKind = phrase && !instructions ? "phrase" : "ai";
  const bodyContent = kind === "phrase" ? phrase : instructions;

  return {
    name: nameSection || fmName,
    description: descSection || fmDesc,
    triggers,
    kind,
    body: bodyContent,
  };
}

/** Assemble sections back into canonical markdown for save. Writes exactly
 *  one of `## Instructions` or `## Phrase` depending on the kind, so the
 *  file matches what the user sees in the editor. */
function assembleMarkdown(s: Sections): string {
  const parts: string[] = [];
  parts.push(`## Name\n${s.name.trim()}`);
  parts.push(`## Description\n${s.description.trim()}`);
  const triggerLines = s.triggers
    .split("\n")
    .map((l) => l.replace(/^\s*[-*]\s*/, "").trim())
    .filter((l) => l.length > 0);
  parts.push(
    `## Triggers\n${triggerLines.map((t) => `- ${t}`).join("\n")}`,
  );
  const heading = s.kind === "phrase" ? "Phrase" : "Instructions";
  parts.push(`## ${heading}\n${s.body.trim()}`);
  return parts.join("\n\n") + "\n";
}

/** Validate a skill name against the same rules the Rust backend enforces. */
function validateName(name: string): string | null {
  if (!name) return "Name is required.";
  if (name.length > 40) return "Name must be 40 characters or fewer.";
  const first = name[0];
  if (!/[a-z]/.test(first)) {
    return "Must start with a lowercase letter.";
  }
  if (name.endsWith("-")) return "Can't end with a hyphen.";
  const bad = name.match(/[^a-z0-9-]/);
  if (bad) {
    return `Only lowercase letters, digits, and hyphens (got "${bad[0]}").`;
  }
  return null;
}

// ── exported panes ────────────────────────────────────────────────────────
//
// Both panes are the same list/editor flow — just filtered and relabelled.
// Exporting two thin wrappers so `SettingsApp` can route to either.

export function SkillsPane() {
  return <KindPane kind="ai" />;
}

export function PhrasesPane() {
  return <KindPane kind="phrase" />;
}

// ── the shared pane ───────────────────────────────────────────────────────

function KindPane({ kind }: { kind: SkillKind }) {
  const copy = COPY[kind];
  const [skills, setSkills] = useState<Skill[]>([]);
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const all = await invoke<Skill[]>("skill_list");
      setSkills(all.filter((s) => s.kind === kind));
    } catch (e) {
      setErr(String(e));
    }
  }, [kind]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const setEnabled = async (name: string, enabled: boolean) => {
    // Optimistic update so the toggle feels instant.
    setSkills((cur) =>
      cur.map((s) => (s.name === name ? { ...s, enabled } : s)),
    );
    try {
      await invoke("skill_set_enabled", { name, enabled });
    } catch (e) {
      setErr(String(e));
      refresh(); // resync on error
    }
  };

  if (mode.kind === "edit") {
    return (
      <SkillEditor
        kind={kind}
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
        kind={kind}
        onClose={(changed) => {
          setMode({ kind: "list" });
          if (changed) refresh();
        }}
      />
    );
  }

  return (
    <>
      <div className="pane-header-row">
        <h1>{copy.title}</h1>
        <button
          type="button"
          className="secondary"
          onClick={() => setMode({ kind: "create" })}
        >
          {copy.newBtn}
        </button>
      </div>
      <p className="subtle">{copy.subtitle}</p>

      {skills.length === 0 ? (
        <div className="empty-hint">{copy.emptyHint}</div>
      ) : (
        <ul className="row-list">
          {skills.map((s) => (
            <li
              key={s.name}
              className={`row row-clickable skill-row${s.enabled ? "" : " skill-row-off"}`}
              onClick={() => setMode({ kind: "edit", name: s.name })}
            >
              <div className="row-main">
                <div className="row-title">
                  {s.name}
                  {!s.enabled && <span className="subtle"> · off</span>}
                </div>
                <div className="subtle">{s.description}</div>
              </div>
              <Toggle
                checked={s.enabled}
                label={s.enabled ? "On" : "Off"}
                onChange={(next) => setEnabled(s.name, next)}
              />
            </li>
          ))}
        </ul>
      )}

      {err && <div className="pane-error">{err}</div>}
    </>
  );
}

// ── compact switch ────────────────────────────────────────────────────────

function Toggle({
  checked,
  label,
  onChange,
}: {
  checked: boolean;
  label: string;
  onChange: (next: boolean) => void;
}) {
  return (
    <label
      className={`skill-toggle${checked ? " on" : ""}`}
      onClick={(e) => e.stopPropagation()}
      title={checked ? "On — click to turn off" : "Off — click to turn on"}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="skill-toggle-track">
        <span className="skill-toggle-thumb" />
      </span>
      <span className="skill-toggle-label">{label}</span>
    </label>
  );
}

// ── editor for an existing skill ──────────────────────────────────────────

function SkillEditor({
  kind,
  name,
  skill,
  onClose,
}: {
  kind: SkillKind;
  name: string;
  skill: Skill | null;
  onClose: (changed: boolean) => void;
}) {
  const copy = COPY[kind];
  const [sections, setSections] = useState<Sections>(emptySections(kind));
  const [initial, setInitial] = useState<Sections>(emptySections(kind));
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [submitAttempted, setSubmitAttempted] = useState(false);

  const nameEditable = !!skill;

  useEffect(() => {
    (async () => {
      try {
        const src = await invoke<string>("skill_get_source", { name });
        const parsed = parseSections(src);
        setSections(parsed);
        setInitial(parsed);
      } catch (e) {
        setErr(String(e));
      }
    })();
  }, [name]);

  const dirty = !sectionsEqual(sections, initial);

  const save = async (next: Sections) => {
    setSubmitAttempted(true);
    const err = preSaveError(next, copy);
    if (err) {
      setErr(err);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      const payloadName = nameEditable ? next.name : name;
      const md = assembleMarkdown({ ...next, name: payloadName });
      await invoke("skill_save", { name, markdown: md });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  const deleteSkill = async () => {
    if (!window.confirm(copy.deleteConfirm(name))) return;
    try {
      await invoke("skill_delete", { name });
      onClose(true);
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <>
      <div className="editor-header">
        <button type="button" className="back-btn" onClick={() => onClose(false)}>
          ← Back
        </button>
        <h1>{name}</h1>
      </div>

      <SkillForm
        kind={kind}
        sections={sections}
        onChange={setSections}
        nameEditable={nameEditable}
        showErrors={submitAttempted}
      />

      {err && <div className="pane-error">{err}</div>}

      <div className="editor-actions">
        <button
          type="button"
          className="primary"
          disabled={!dirty || saving}
          onClick={() => save(sections)}
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
        {skill && (
          <button type="button" className="danger-btn" onClick={deleteSkill}>
            Delete
          </button>
        )}
      </div>
    </>
  );
}

// ── new-skill form ─────────────────────────────────────────────────────────

function SkillCreator({
  kind,
  onClose,
}: {
  kind: SkillKind;
  onClose: (changed: boolean) => void;
}) {
  const copy = COPY[kind];
  const [sections, setSections] = useState<Sections>(emptySections(kind));
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [submitAttempted, setSubmitAttempted] = useState(false);

  const save = async () => {
    setSubmitAttempted(true);
    const err = preSaveError(sections, copy);
    if (err) {
      setErr(err);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      const md = assembleMarkdown(sections);
      await invoke<string>("skill_create", { markdown: md });
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
          ← Back
        </button>
        <h1>{copy.editorNewTitle}</h1>
      </div>

      <SkillForm
        kind={kind}
        sections={sections}
        onChange={setSections}
        nameEditable={true}
        showErrors={submitAttempted}
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

// ── shared form component ─────────────────────────────────────────────────

function SkillForm({
  kind,
  sections,
  onChange,
  nameEditable,
  showErrors,
}: {
  kind: SkillKind;
  sections: Sections;
  onChange: (next: Sections) => void;
  nameEditable: boolean;
  showErrors: boolean;
}) {
  const copy = COPY[kind];
  const patch = (k: keyof Sections) => (v: string) =>
    onChange({ ...sections, [k]: v });

  const nameErr = useMemo(() => validateName(sections.name), [sections.name]);
  const triggersErr =
    showErrors && !sections.triggers.trim()
      ? "Add at least one trigger phrase so it can activate."
      : null;
  const bodyErr =
    showErrors && !sections.body.trim() ? copy.bodyRequiredError : null;
  const descriptionErr =
    showErrors && !sections.description.trim()
      ? "Description is required."
      : null;

  return (
    <div className="skill-form">
      <Field
        label="Name"
        required
        hint="Lowercase letters, digits, hyphens."
        error={nameEditable && (sections.name || showErrors) ? nameErr : null}
      >
        <input
          type="text"
          className="text-input"
          value={sections.name}
          onChange={(e) => patch("name")(e.target.value)}
          placeholder={kind === "phrase" ? "my-phrase" : "my-skill"}
          spellCheck={false}
          autoComplete="off"
          disabled={!nameEditable}
        />
      </Field>

      <Field
        label="Description"
        required
        hint="One line shown under this row in the list."
        error={descriptionErr}
      >
        <input
          type="text"
          className="text-input"
          value={sections.description}
          onChange={(e) => patch("description")(e.target.value)}
          placeholder={
            kind === "phrase"
              ? "Paste my Calendly booking link"
              : "Turn dictation into a slack message"
          }
          spellCheck={false}
        />
      </Field>

      <Field
        label="Trigger phrases"
        required
        hint={
          <>
            One phrase per line. Use <code>&lt;variable&gt;</code> to capture
            parts of what the user said.
          </>
        }
        error={triggersErr}
      >
        <textarea
          className="text-area"
          rows={3}
          value={sections.triggers}
          onChange={(e) => patch("triggers")(e.target.value)}
          placeholder={
            kind === "phrase"
              ? "calendly\nsend calendly\nbook time"
              : "slack <recipient> <body>\nmessage <recipient> <body>"
          }
          spellCheck={false}
        />
      </Field>

      <Field
        label={copy.bodyFieldLabel}
        required
        hint={copy.bodyFieldHint}
        error={bodyErr}
      >
        <textarea
          className="text-area mono"
          rows={7}
          value={sections.body}
          onChange={(e) => patch("body")(e.target.value)}
          placeholder={copy.bodyPlaceholder}
          spellCheck={false}
        />
      </Field>
    </div>
  );
}

function Field({
  label,
  hint,
  required,
  error,
  children,
}: {
  label: string;
  hint?: React.ReactNode;
  required?: boolean;
  error?: string | null;
  children: React.ReactNode;
}) {
  return (
    <div className={`skill-field ${error ? "has-error" : ""}`}>
      <div className="skill-field-label">
        <span>
          {label}
          {required && <span className="req-mark"> *</span>}
        </span>
      </div>
      {hint && <div className="skill-field-hint">{hint}</div>}
      {children}
      {error && <div className="skill-field-error">{error}</div>}
    </div>
  );
}

// ── helpers ────────────────────────────────────────────────────────────────

/** Return a human-readable error when `s` is not yet ready to save, or null. */
function preSaveError(
  s: Sections,
  copy: (typeof COPY)[SkillKind],
): string | null {
  const nameErr = validateName(s.name);
  if (nameErr) return nameErr;
  if (!s.description.trim()) return "Description is required.";
  if (!s.triggers.trim()) {
    return "Add at least one trigger phrase.";
  }
  if (!s.body.trim()) return copy.bodyRequiredError;
  return null;
}
