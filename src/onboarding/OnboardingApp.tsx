import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ollamaLogo from "../assets/ollama.png";
import "./onboarding.css";
import { OllamaInstallPills, OllamaInstallVerifyTipsContent } from "../components/OllamaInstallStatus";

// ── Types ──────────────────────────────────────────────────────────────────

interface OnboardingStatus {
  model_cached: boolean;
  model_downloading: boolean;
  model_download_pct: number | null;
  mic_permission: "granted" | "denied" | "unknown";
  accessibility: boolean;
  /** False when the running binary's codesign identifier isn't `com.soll.app`
   *  (e.g. an un-re-signed local `pnpm tauri build` produces a hash-based
   *  identifier). When false AND accessibility is pending, the user's
   *  System Settings grant can't apply — we surface a "Reset & re-grant"
   *  hint instead of looping on the generic restart message. */
  signing_identifier_ok: boolean;
  ollama_running: boolean;
  ollama_active_model_pulled: boolean;
  ollama_installed: boolean;
  ollama_app_installed: boolean;
  ollama_cli_installed: boolean;
  has_dictated: boolean;
  has_skills: boolean;
  dismissed: boolean;
}

type StepState = "done" | "in_progress" | "denied" | "pending";

// (Old `StepDef` interface removed in the conversational redesign — each
// step now renders its own JSX directly via the Step1Whisper / Step2Mic / ...
// functions in this file. `StepMeta` further down captures just the
// state-for-progress info that OnboardingApp needs.)

// ── Icons ──────────────────────────────────────────────────────────────────

const ICONS: Record<string, React.ReactNode> = {
  model: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round">
      <rect x="1"  y="10" width="2" height="4"  rx="1"/>
      <rect x="5"  y="7"  width="2" height="10" rx="1"/>
      <rect x="9"  y="3"  width="2" height="18" rx="1"/>
      <rect x="13" y="3"  width="2" height="18" rx="1"/>
      <rect x="17" y="7"  width="2" height="10" rx="1"/>
      <rect x="21" y="10" width="2" height="4"  rx="1"/>
    </svg>
  ),
  mic: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="2" width="6" height="12" rx="3"/>
      <path d="M5 11a7 7 0 0 0 14 0"/>
      <line x1="12" y1="18" x2="12" y2="22"/>
      <line x1="8"  y1="22" x2="16" y2="22"/>
    </svg>
  ),
  accessibility: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="4" r="1.8" fill="currentColor" stroke="none"/>
      <line x1="4" y1="9" x2="20" y2="9"/>
      <path d="M12 9v5l-3 7"/>
      <path d="M12 14l3 7"/>
    </svg>
  ),
  ollama: <img src={ollamaLogo} className="ob-step-icon-img" alt="Ollama"/>,
  dictation: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>
    </svg>
  ),
};

// ── Helpers ────────────────────────────────────────────────────────────────

// (Helpers `cssState` and `StatusBadge` were removed in the conversational
// redesign. The status pill is gone — "done" is signalled via the
// success-tinted icon and the dot strip at the bottom of the wizard.)

// ── Sub-components ─────────────────────────────────────────────────────────

interface ModelInfo {
  id: string;
  label: string;
  size: string;
  is_cached: boolean;
  is_active: boolean;
  is_downloading: boolean;
  is_recommended: boolean;
}

function ModelPicker({ models, downloadPct }: {
  models: ModelInfo[];
  /** Global download % from `onboarding_status` — applies to whichever
   *  model has `is_downloading=true`. Captured into `pausedPctMap` on
   *  pause so we can show "Paused at X%" after the backend's pct clears. */
  downloadPct: number | null;
}) {
  // Optimistic UI state:
  //
  //   `pendingCancelId` — the user just clicked pause; backend hasn't
  //     confirmed yet. Status shows "Pausing…" briefly.
  //
  //   `pausedPctMap` — last-seen percent for any model the user paused.
  //     The backend zeroes its progress on cancel, but we want to show
  //     "Paused at 42%". Snapshot at click time, clear when the model
  //     becomes cached or starts downloading again.
  const [pendingCancelId, setPendingCancelId] = useState<string | null>(null);
  const [pausedPctMap, setPausedPctMap] = useState<Record<string, number>>({});

  useEffect(() => {
    if (pendingCancelId) {
      const m = models.find(x => x.id === pendingCancelId);
      if (!m || !m.is_downloading) setPendingCancelId(null);
    }
  }, [models, pendingCancelId]);

  // Clear a model's paused-at memory once it's no longer in the
  // "downloaded-partially, not currently downloading" state.
  useEffect(() => {
    setPausedPctMap(prev => {
      const out: Record<string, number> = {};
      for (const [id, pct] of Object.entries(prev)) {
        const m = models.find(x => x.id === id);
        if (m && !m.is_cached && !m.is_downloading) {
          out[id] = pct;
        }
      }
      return out;
    });
  }, [models]);

  // Multiple models can be cached. The toggle on a chip reflects "this is
  // the active speech model" — i.e. cached AND active. Click rules:
  //   • Already active+cached → no-op (use Delete button below to remove).
  //   • Cached but inactive → switch to it (no download).
  //   • Not cached → start download; becomes active when finished.
  //   • Downloading → pause (cancel + keep .part for later resume).
  //   • Paused (.part exists, not downloading) → resume from where it
  //     paused, courtesy of `ensure_model`'s Range-based resume.
  async function handleClick(m: ModelInfo) {
    if (m.is_downloading) {
      // Snapshot the current % so we can show "Paused at X%" after
      // the backend clears its progress counters.
      if (downloadPct != null) {
        setPausedPctMap(prev => ({ ...prev, [m.id]: downloadPct }));
      }
      setPendingCancelId(m.id);
      await invoke("model_cancel_download");
      return;
    }
    if (m.is_cached && m.is_active) return;
    if (m.is_cached) {
      // Switch active to this already-cached model. No download.
      await invoke("model_activate", { id: m.id });
      return;
    }
    await invoke("model_select", { id: m.id });
    void invoke("model_download", { id: m.id });
  }

  return (
    <div className="ob-model-picker ob-model-picker--compact">
      {models.map(m => {
        const isCancelling = pendingCancelId === m.id;
        const pausedAt = pausedPctMap[m.id];
        const isPaused = !m.is_downloading && !isCancelling && !m.is_cached && pausedAt != null;
        const on = (m.is_cached && m.is_active) || (m.is_downloading && !isCancelling);
        // Disable only the chip that's already active+cached — clicking it
        // would be a no-op anyway. Every other chip stays clickable so the
        // user can switch freely, download an additional one, or pause a
        // download.
        const disabled = m.is_cached && m.is_active;
        // Compact status: ONLY show text when there's something
        // worth saying. Cached + clickable doesn't need a "click to
        // use" hint — the toggle visual already implies the action.
        // The previous status line ("Cached · click to use" on three
        // of four cards) was clutter that ate vertical space on Step 1.
        const status =
          isCancelling ? "Pausing…"
          : m.is_downloading ? "Downloading…"
          : isPaused ? `Paused at ${pausedAt}%`
          : !m.is_cached ? "Not downloaded"
          : null;
        return (
          <button
            key={m.id}
            type="button"
            className={`ob-model-card${on ? " ob-model-card--on" : ""}`}
            disabled={disabled}
            onClick={() => { void handleClick(m); }}
          >
            <div className="ob-model-card-info">
              <div className="ob-model-card-name">{m.label}</div>
              <div className="ob-model-card-size">{m.size}</div>
              {status && <div className="ob-model-card-pulled">{status}</div>}
            </div>
            <span className={`ob-model-card-toggle${on ? " ob-model-card-toggle--on" : ""}`} />
          </button>
        );
      })}
    </div>
  );
}

interface OllamaModelInfo {
  tag: string;
  display_name: string;
  author: string;
  size: string;
  is_active: boolean;
  is_pulled: boolean;
  /** Streaming pull progress 0–100, or null when no pull is active for
   *  this model. Populated by the streaming Ollama pull task in Rust. */
  pull_pct: number | null;
}

function OllamaModelPicker({ models, ollamaRunning, pullingTag, onPullStart, onPullCancel }: {
  models: OllamaModelInfo[];
  ollamaRunning: boolean;
  pullingTag: string | null;
  onPullStart: (tag: string) => void;
  onPullCancel: () => void;
}) {
  // Optimistic UI state:
  //
  //   `pendingCancelTag` — the user just clicked pause but the backend
  //     hasn't confirmed yet. Status shows "Pausing…" briefly.
  //
  //   `pausedPctMap` — last-seen `pull_pct` for any tag the user paused.
  //     Backend zeroes `pull_pct` on cancel, but we want to show "Paused
  //     at 42%" not "Paused at –". So we snapshot the pct *before* sending
  //     the cancel command. Cleared when the user resumes or the model
  //     finishes pulling.
  const [pendingCancelTag, setPendingCancelTag] = useState<string | null>(null);
  const [pausedPctMap, setPausedPctMap] = useState<Record<string, number>>({});

  // Clear pendingCancelTag once the backend confirms (pull_pct → null).
  useEffect(() => {
    if (!pendingCancelTag) return;
    const m = models.find(x => x.tag === pendingCancelTag);
    if (!m || m.pull_pct == null) setPendingCancelTag(null);
  }, [models, pendingCancelTag]);

  // Clear a model's paused-at memory when it's fully pulled (download
  // completed) or when a new pull is already underway for it.
  useEffect(() => {
    setPausedPctMap(prev => {
      const out: Record<string, number> = {};
      for (const [tag, pct] of Object.entries(prev)) {
        const m = models.find(x => x.tag === tag);
        // Keep the paused-at marker as long as the model is neither
        // already pulled nor actively pulling.
        if (m && !m.is_pulled && m.pull_pct == null) {
          out[tag] = pct;
        }
      }
      return out;
    });
  }, [models]);

  // Radio-button semantics: exactly one chip is "on" at a time — the active
  // model that Soll uses for cleanup. `is_pulled` is shown as a separate
  // status badge so the user can see what's already on disk without it
  // overriding the active selection. Clicking a *currently-pulling* chip
  // pauses the pull (Ollama keeps the blob layers it already downloaded
  // server-side, so the next click resumes from there).
  async function handleClickChip(m: OllamaModelInfo) {
    if (!ollamaRunning) return;
    // Block if a *different* model is pulling — Ollama doesn't parallelise
    // pulls and our progress tracking only supports one in-flight tag.
    // Don't block on `pullingTag === m.tag` since that just means *this*
    // model is the one in flight (or was — could be the stale optimistic
    // flag from before a pause).
    if (pullingTag && pullingTag !== m.tag) return;
    // Click on the actively-pulling chip → pause it. Snapshot the
    // percentage NOW so we can show "Paused at X%" — once we send the
    // cancel command, `pull_pct` clears on the backend and we lose it.
    if (m.pull_pct != null) {
      setPausedPctMap(prev => ({ ...prev, [m.tag]: m.pull_pct as number }));
      setPendingCancelTag(m.tag);
      // Tell the parent to drop its optimistic pullingTag flag too,
      // otherwise the next click (to resume) might get mis-cleared by
      // the parent's "pull_pct === null → clear" logic before the
      // backend has time to bump pull_pct off zero.
      onPullCancel();
      await invoke("ollama_cancel_pull");
      return;
    }
    if (m.is_active && m.is_pulled) return; // already the default — no-op
    // Either a fresh pull or a resume — both go through model_set then
    // pull_active. Resume works because Ollama's server kept the partial
    // blob layers from the earlier session.
    await invoke("ollama_model_set", { tag: m.tag });
    if (!m.is_pulled) {
      onPullStart(m.tag);
      void invoke("ollama_pull_active");
    }
  }

  return (
    <div className="ob-model-picker">
      {models.map(m => {
        const isPulling = m.pull_pct != null || pullingTag === m.tag;
        const isCancelling = pendingCancelTag === m.tag;
        const pausedAt = pausedPctMap[m.tag];
        const isPaused = !isPulling && !isCancelling && !m.is_pulled && pausedAt != null;
        const on = (m.is_active && m.is_pulled) || (isPulling && !isCancelling);
        const disabled = !ollamaRunning || (pullingTag !== null && pullingTag !== m.tag);
        const statusLabel = isCancelling
          ? "Pausing…"
          : isPulling
          ? (m.pull_pct != null ? `Pulling… ${m.pull_pct}% — click to pause` : "Pulling…")
          : isPaused
          ? `Paused at ${pausedAt}% — click to resume`
          : m.is_pulled
          ? (m.is_active ? null : "Downloaded")
          : "Not downloaded";
        return (
          <button
            key={m.tag}
            type="button"
            className={`ob-model-card${on ? " ob-model-card--on" : ""}`}
            disabled={disabled}
            onClick={() => { void handleClickChip(m); }}
          >
            <div className="ob-model-card-info">
              <div className="ob-model-card-name">{m.display_name}</div>
              <div className="ob-model-card-size">{m.size} · {m.author}</div>
              {statusLabel && (
                <div className="ob-model-card-pulled">{statusLabel}</div>
              )}
            </div>
            <span className={`ob-model-card-toggle${on ? " ob-model-card-toggle--on" : ""}`} />
          </button>
        );
      })}
    </div>
  );
}

function DictationTest({ status }: { status: OnboardingStatus }) {
  const [value, setValue] = useState("");

  const blockers: string[] = [];
  if (!status.model_cached) blockers.push("Speech model isn't downloaded — see Step 1.");
  if (status.mic_permission !== "granted") blockers.push("Microphone access not granted — see Step 2.");
  if (!status.accessibility) blockers.push("Accessibility access not granted — see Step 3.");

  const ready = blockers.length === 0;
  const placeholder = !ready
    ? "Resolve the issues below first."
    : status.has_dictated
    ? "✓ Dictation works. Try another sentence to feel the flow."
    : "Click here, then hold ⌃⇧Space and speak. Release when done — your words appear here.";

  return (
    <div className="ob-dictation-test">
      <textarea
        className="ob-dictation-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={placeholder}
        rows={2}
        disabled={!ready}
      />
      {!ready ? (
        <div className="ob-dictation-blockers">
          <strong>Blocking issues</strong>
          <ul>{blockers.map((b, i) => <li key={i}>{b}</li>)}</ul>
        </div>
      ) : !status.has_dictated ? (
        <p className="ob-dictation-hint">
          Tip: Soll pastes into whichever field has focus. Click into the box first, then trigger the shortcut.
        </p>
      ) : null}
    </div>
  );
}

// (Old `ActionButton` removed — replaced by PrimaryButton/SecondaryLink
// in the conversational redesign.)

// (The old `OllamaStatusPanel` and `Toggle` components were removed
// during the conversational redesign. The Ollama-step copy is now
// inlined into `Step4Ollama` so the case-by-case logic lives next to
// the primary CTA it drives; the iOS-style toggle didn't carry over
// because each step now has a single primary button instead.)

// ── Step metadata (for nav + progress, no rendering) ───────────────────────

interface StepMeta {
  id: string;
  /** Short label for dot tooltips + sidebar. Not shown on the step itself. */
  shortTitle: string;
  state: StepState;
  /** True when the step doesn't block "All Done". Right now only Ollama. */
  optional?: boolean;
}

function deriveStepMeta(
  s: OnboardingStatus,
  ollamaModels: OllamaModelInfo[],
  pullingOllamaTag: string | null,
): StepMeta[] {
  const modelState: StepState = s.model_cached ? "done"
    : s.model_downloading ? "in_progress" : "pending";
  const micState: StepState = s.mic_permission === "granted" ? "done"
    : s.mic_permission === "denied" ? "denied" : "pending";
  const axState: StepState = s.accessibility ? "done" : "pending";
  // Ollama is "done" only when the *active* model is pulled — having
  // some other model on disk doesn't help if Llama is active and missing.
  const activeOllamaModel = ollamaModels.find(m => m.is_active);
  const ollamaState: StepState =
    s.ollama_running && activeOllamaModel?.is_pulled ? "done"
    : pullingOllamaTag ? "in_progress"
    : "pending";
  const dictState: StepState = s.has_dictated ? "done" : "pending";

  return [
    { id: "model", shortTitle: "Speech model", state: modelState },
    { id: "mic", shortTitle: "Microphone", state: micState },
    { id: "ax", shortTitle: "Accessibility", state: axState },
    { id: "ollama", shortTitle: "AI cleanup", state: ollamaState, optional: true },
    { id: "dictation", shortTitle: "First dictation", state: dictState },
  ];
}

// ── Conversational step layout ─────────────────────────────────────────────
//
// Single skeleton every step renders into — keeps visual rhythm consistent
// no matter what's happening internally. Slots, top to bottom:
//
//   • Big centered icon
//   • Plain-English title (sentence case)
//   • 1–2 line subtitle in muted text
//   • Optional `children` (pickers, dictation textarea — content above CTA)
//   • The ONE primary action — large, prominent, only thing you'd click
//   • Secondary links — tiny, link-style, below the primary CTA
//
// All five steps use exactly this. Anything else (badges, multiple buttons,
// per-step layouts) is intentionally absent so we don't drift back into
// "every step looks different" land.

function ConversationalStep({
  header, icon, title, subtitle, primary, secondary, children, success,
}: {
  /** Optional sub-step status panel rendered above everything else — used
   *  by Step 4 to always show "1. Install Ollama / 2. Pick a model" so
   *  the user can see both pieces of the setup at a glance no matter
   *  which sub-state they're currently in. */
  header?: React.ReactNode;
  icon: React.ReactNode;
  title: string;
  subtitle: React.ReactNode;
  /** The ONE primary action. Null when the step is in a terminal "done" view
   *  and the user just clicks Next in the footer. */
  primary?: React.ReactNode;
  /** Small link-style alternatives below the primary action. */
  secondary?: React.ReactNode;
  /** Pre-CTA content — picker UIs, the dictation textarea, etc. */
  children?: React.ReactNode;
  /** Tints the icon green to signal "this step is done". */
  success?: boolean;
}) {
  return (
    <div className={`cs-step${success ? " cs-step--success" : ""}`}>
      {header && <div className="cs-header">{header}</div>}
      <div className="cs-icon-wrap">{icon}</div>
      <h1 className="cs-title">{title}</h1>
      <div className="cs-subtitle">{subtitle}</div>
      {children && <div className="cs-content">{children}</div>}
      {primary && <div className="cs-primary-slot">{primary}</div>}
      {secondary && <div className="cs-secondary-slot">{secondary}</div>}
    </div>
  );
}

/// Two-row sub-step status panel for Step 4 (Ollama). Always visible
/// when the user is on Step 4, regardless of which sub-state they're in
/// (not-installed / installed-not-running / picking model / pulling /
/// done). Makes the user's mental model — "I need to (1) install Ollama
/// and (2) pick a model" — visible at all times.
///
/// Sub-step 1's detail row uses **two pills** (one per install shape)
/// rather than a single text label, so the user can see at a glance
/// which shape(s) they have — present pills are tinted green with a ✓,
/// absent ones are muted with an em-dash. Earlier versions just said
/// "Menu-bar app + CLI" which was technically correct but hard to scan.
function OllamaSubSteps({ status, activeModel }: {
  status: OnboardingStatus;
  activeModel?: OllamaModelInfo;
}) {
  // Verify-tips open/close state is owned here (not by the disclosure
  // component) so the trigger can be placed inline with the sub-step
  // title — i.e. "Install Ollama  ▸ verify" all on one row — while the
  // expandable content still renders below the pills.
  const [verifyOpen, setVerifyOpen] = useState(false);
  const installDone = status.ollama_installed;
  const modelDone = !!activeModel?.is_pulled;

  return (
    <div className="ob-substeps-panel">
      <ol className="ob-substeps">
        {/* Sub-step 1: two visible lines —
              • Line 1: "Install Ollama" + inline "▸ verify" trigger
              • Line 2: install-shape pills ([✓ App] [✓ CLI])
            When verify is clicked the tips render below this panel
            (full width, see `<div className="ob-substeps-verify">`
            below) — not inside this column. That keeps both substep
            columns the same height. */}
        <li className={installDone ? "ob-substep--done" : "ob-substep--todo"}>
          <span className="ob-substep-mark">{installDone ? "✓" : "1"}</span>
          <div className="ob-substep-text">
            <div className="ob-substep-title-row">
              <span className="ob-substep-title">Install Ollama</span>
              <button
                type="button"
                className="ob-substep-verify-link"
                onClick={() => setVerifyOpen(o => !o)}
                aria-expanded={verifyOpen}
              >
                {verifyOpen ? "▾" : "▸"} verify
              </button>
            </div>
            <OllamaInstallPills
              appInstalled={status.ollama_app_installed}
              cliInstalled={status.ollama_cli_installed}
            />
          </div>
        </li>
        {/* Sub-step 2: title + active model rendered as a chip (matches
            the App/CLI pill style on sub-step 1 for visual consistency).
            When no model is pulled yet, falls back to plain detail text
            because a "Not yet" chip would be redundant with the muted
            mark on the left. */}
        <li className={modelDone ? "ob-substep--done" : "ob-substep--todo"}>
          <span className="ob-substep-mark">{modelDone ? "✓" : "2"}</span>
          <div className="ob-substep-text">
            <span className="ob-substep-title">Pick an AI model</span>
            {modelDone ? (
              <div className="oi-pills">
                <span className="oi-pill oi-pill--present">
                  <span className="oi-pill-mark">✓</span>
                  {activeModel!.display_name}
                </span>
              </div>
            ) : (
              <span className="ob-substep-detail">
                {activeModel
                  ? `${activeModel.display_name} — not downloaded`
                  : "Not yet — pick one below"}
              </span>
            )}
          </div>
        </li>
      </ol>
      {/* Full-width verify info box. Lives outside the 2-col substeps
          grid so it spans the entire panel width when opened, instead
          of squeezing into the left column. Divider above visually
          separates it from the substep rows. */}
      {verifyOpen && (
        <div className="ob-substeps-verify">
          <OllamaInstallVerifyTipsContent
            appInstalled={status.ollama_app_installed}
            cliInstalled={status.ollama_cli_installed}
          />
        </div>
      )}
    </div>
  );
}

// `InstallPill` moved to the shared `OllamaInstallStatus` component
// (`src/components/OllamaInstallStatus.tsx`) so both the Setup Guide
// and Settings → Models render the same widget.

function PrimaryButton({ onClick, disabled, danger, children }: {
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={`cs-primary-btn${danger ? " cs-primary-btn--danger" : ""}`}
      onClick={() => { void onClick(); }}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

function SecondaryLink({ onClick, children }: {
  onClick: () => void | Promise<void>;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className="cs-secondary-link"
      onClick={() => { void onClick(); }}
    >
      {children}
    </button>
  );
}

// ── Per-step renderers ─────────────────────────────────────────────────────

interface StepProps {
  status: OnboardingStatus;
  models: ModelInfo[];
  ollamaModels: OllamaModelInfo[];
  pullingOllamaTag: string | null;
  setPullingOllamaTag: (v: string | null) => void;
  onContinue: () => void;
}

function Step1Whisper({ status, models, onContinue }: StepProps) {
  const recommended = models.find(m => m.is_recommended) ?? models[0];
  const downloading = models.find(m => m.is_downloading);
  const cachedActive = models.find(m => m.is_active && m.is_cached);
  const [showAll, setShowAll] = useState(false);

  // ── Terminal: model is downloaded and active ─────────────────────────────
  if (cachedActive) {
    return (
      <ConversationalStep
        icon={ICONS.model}
        success
        title="Speech recognition is ready"
        subtitle={<>
          <strong>{cachedActive.label}</strong> ({cachedActive.size}) is downloaded
          and loaded. Soll uses Whisper to turn speech into text — all on your Mac.
        </>}
        primary={
          <PrimaryButton onClick={onContinue}>Continue</PrimaryButton>
        }
        secondary={
          <SecondaryLink onClick={() => setShowAll(s => !s)}>
            {showAll ? "Hide model options" : "Use a different model size"}
          </SecondaryLink>
        }
      >
        {showAll && <ModelPicker models={models} downloadPct={status.model_download_pct} />}
      </ConversationalStep>
    );
  }

  // ── Active: a download is in flight right now ────────────────────────────
  if (downloading) {
    const pct = status.model_download_pct;
    return (
      <ConversationalStep
        icon={ICONS.model}
        title={`Downloading ${downloading.label}…`}
        subtitle={pct != null
          ? `${pct}% complete. Safe to leave this window open — Soll will continue in the background.`
          : "Starting download…"}
        primary={
          <PrimaryButton onClick={() => invoke("model_cancel_download")}>
            Pause download
          </PrimaryButton>
        }
        secondary={
          <SecondaryLink onClick={() => setShowAll(s => !s)}>
            {showAll ? "Hide model options" : "Switch to a different size"}
          </SecondaryLink>
        }
      >
        {showAll && <ModelPicker models={models} downloadPct={pct} />}
      </ConversationalStep>
    );
  }

  // ── Initial: no model downloaded yet ─────────────────────────────────────
  return (
    <ConversationalStep
      icon={ICONS.model}
      title="First, let's set up speech recognition"
      subtitle={<>
        Soll uses <strong>Whisper</strong> — a small AI model that converts your
        speech into text. Everything runs on your Mac. We recommend the{" "}
        <strong>{recommended.label}</strong> model: best quality on Apple Silicon.
      </>}
      primary={
        <PrimaryButton onClick={async () => {
          await invoke("model_select", { id: recommended.id });
          void invoke("model_download", { id: recommended.id });
        }}>
          Download {recommended.label} ({recommended.size})
        </PrimaryButton>
      }
      secondary={
        <SecondaryLink onClick={() => setShowAll(s => !s)}>
          {showAll ? "Hide model options" : "Choose a different size"}
        </SecondaryLink>
      }
    >
      {showAll && <ModelPicker models={models} downloadPct={status.model_download_pct} />}
    </ConversationalStep>
  );
}

function Step2Mic({ status, onContinue }: StepProps) {
  const state = status.mic_permission;

  if (state === "granted") {
    return (
      <ConversationalStep
        icon={ICONS.mic}
        success
        title="Microphone access granted"
        subtitle="Soll can now hear you when you hold the dictation shortcut."
        primary={<PrimaryButton onClick={onContinue}>Continue</PrimaryButton>}
        secondary={
          <SecondaryLink onClick={() => invoke("open_privacy_settings", { section: "Privacy_Microphone" })}>
            Manage in System Settings
          </SecondaryLink>
        }
      />
    );
  }

  // macOS only shows the request dialog once. After "denied", the request
  // API returns instantly without re-prompting — only path back is System
  // Settings. So change the primary CTA accordingly.
  if (state === "denied") {
    return (
      <ConversationalStep
        icon={ICONS.mic}
        title="Microphone access was declined"
        subtitle="macOS won't show its permission dialog a second time. Open System Settings → Privacy → Microphone and toggle Soll on."
        primary={
          <PrimaryButton onClick={() => invoke("open_privacy_settings", { section: "Privacy_Microphone" })}>
            Open System Settings
          </PrimaryButton>
        }
      />
    );
  }

  // pending — fresh request
  return (
    <ConversationalStep
      icon={ICONS.mic}
      title="Soll needs to hear you"
      subtitle="Grant microphone access so Soll can record your voice when you hold the dictation shortcut. Audio is processed locally — never sent to a server."
      primary={
        <PrimaryButton onClick={() => invoke("request_mic_permission")}>
          Grant microphone access
        </PrimaryButton>
      }
      secondary={
        <SecondaryLink onClick={() => invoke("open_privacy_settings", { section: "Privacy_Microphone" })}>
          Or open System Settings directly
        </SecondaryLink>
      }
    />
  );
}

function Step3Accessibility({ status, onContinue }: StepProps) {
  if (status.accessibility) {
    return (
      <ConversationalStep
        icon={ICONS.accessibility}
        success
        title="Accessibility access granted"
        subtitle="Soll can now paste transcribed text wherever your cursor is."
        primary={<PrimaryButton onClick={onContinue}>Continue</PrimaryButton>}
        secondary={
          <SecondaryLink onClick={() => invoke("open_privacy_settings", { section: "Privacy_Accessibility" })}>
            Manage in System Settings
          </SecondaryLink>
        }
      />
    );
  }

  // Identity-mismatch path: this build's codesign identifier isn't
  // `com.soll.app`, so any toggle the user flips in System Settings is
  // recorded against the wrong identity. Tccutil reset clears the stale
  // grant; the next request creates a fresh entry tied to the running
  // signature. See onboarding.rs::current_signing_identifier.
  if (!status.signing_identifier_ok) {
    return (
      <ConversationalStep
        icon={ICONS.accessibility}
        title="Reset the Accessibility grant"
        subtitle={<>
          This build isn't signed as <code>com.soll.app</code>, so any toggle
          in System Settings is recorded against a different identity and
          doesn't reach this process. Click below to clear the stale grant,
          then toggle Soll on in System Settings and restart.
        </>}
        primary={
          <PrimaryButton onClick={async () => {
            await invoke("reset_accessibility_grant");
            await invoke("open_privacy_settings", { section: "Privacy_Accessibility" });
          }}>
            Reset &amp; open System Settings
          </PrimaryButton>
        }
        secondary={
          <SecondaryLink onClick={() => invoke("restart_app")}>
            Restart Soll to apply
          </SecondaryLink>
        }
      />
    );
  }

  // Normal pending path
  return (
    <ConversationalStep
      icon={ICONS.accessibility}
      title="Soll needs to paste into other apps"
      subtitle={<>
        Grant accessibility access so transcribed text appears wherever your
        cursor is — Slack, Notes, Terminal, anywhere. macOS caches this status
        per process, so you'll need to <strong>restart Soll once</strong> after
        flipping the toggle in System Settings.
      </>}
      primary={
        <PrimaryButton onClick={() => invoke("request_accessibility_permission")}>
          Open System Settings
        </PrimaryButton>
      }
      secondary={
        <SecondaryLink onClick={() => invoke("restart_app")}>
          I&apos;ve granted it — restart Soll to apply
        </SecondaryLink>
      }
    />
  );
}

function Step4Ollama({ status, ollamaModels, pullingOllamaTag, setPullingOllamaTag, onContinue }: StepProps) {
  const activeOllamaModel = ollamaModels.find(m => m.is_active);
  const pullingModel = pullingOllamaTag
    ? ollamaModels.find(m => m.tag === pullingOllamaTag)
    : undefined;
  const recommended = ollamaModels.find(m => m.tag === "llama3.2:3b") ?? ollamaModels[0];

  // Sub-step status panel — rendered above every state below so the
  // user always sees both pieces of the setup ("install Ollama" +
  // "pick a model") with their current state, regardless of which
  // sub-state we're rendering. Closes the long-standing UX gap where
  // the model picker view hid whether Ollama itself was installed.
  const subSteps = <OllamaSubSteps status={status} activeModel={activeOllamaModel} />;

  // ── Terminal: a model is pulled and ready ────────────────────────────────
  if (status.ollama_running && activeOllamaModel?.is_pulled) {
    return (
      <ConversationalStep
        header={subSteps}
        icon={ICONS.ollama}
        success
        title="AI cleanup is ready"
        subtitle={<>
          <strong>{activeOllamaModel.display_name}</strong> is your active
          model. Soll will use it to clean up transcripts and run Skills.
          You can change models anytime in Settings.
        </>}
        primary={<PrimaryButton onClick={onContinue}>Continue</PrimaryButton>}
        secondary={
          <SecondaryLink onClick={async () => {
            if (!confirm(`Delete ${activeOllamaModel.display_name} from Ollama? You'll need to re-pull it before AI cleanup works again.`)) return;
            await invoke("ollama_delete_active");
          }}>
            Delete {activeOllamaModel.display_name}
          </SecondaryLink>
        }
      />
    );
  }

  // ── Active: a pull is in flight ──────────────────────────────────────────
  if (status.ollama_running && pullingModel) {
    return (
      <ConversationalStep
        header={subSteps}
        icon={ICONS.ollama}
        title={`Downloading ${pullingModel.display_name}…`}
        subtitle={pullingModel.pull_pct != null
          ? `${pullingModel.pull_pct}% of ${pullingModel.size}. First pull can take 5–10 minutes — safe to leave this open.`
          : `Starting download of ${pullingModel.size}…`}
        primary={
          <PrimaryButton onClick={async () => {
            await invoke("ollama_cancel_pull");
            setPullingOllamaTag(null);
          }}>
            Pause download
          </PrimaryButton>
        }
      />
    );
  }

  // ── Ollama up, no model pulled yet ───────────────────────────────────────
  if (status.ollama_running && recommended) {
    return (
      <ConversationalStep
        header={subSteps}
        icon={ICONS.ollama}
        title="Pick an AI model"
        subtitle={<>
          Soll uses a small language model to clean up transcripts. We recommend{" "}
          <strong>{recommended.display_name}</strong> — fast, accurate, runs well
          on Apple Silicon. You can change this anytime in Settings.
        </>}
        primary={
          <PrimaryButton onClick={async () => {
            await invoke("ollama_model_set", { tag: recommended.tag });
            setPullingOllamaTag(recommended.tag);
            void invoke("ollama_pull_active");
          }}>
            Download {recommended.display_name} ({recommended.size})
          </PrimaryButton>
        }
        secondary={
          <>
            <SecondaryLink onClick={onContinue}>Skip — I&apos;ll use raw transcripts</SecondaryLink>
          </>
        }
      >
        <OllamaModelPicker
          models={ollamaModels}
          ollamaRunning={status.ollama_running}
          pullingTag={pullingOllamaTag}
          onPullStart={setPullingOllamaTag}
          onPullCancel={() => setPullingOllamaTag(null)}
        />
      </ConversationalStep>
    );
  }

  // ── Ollama not installed at all ──────────────────────────────────────────
  if (!status.ollama_installed) {
    return (
      <ConversationalStep
        header={subSteps}
        icon={ICONS.ollama}
        title="Install Ollama"
        subtitle={<>
          Ollama is a small program that runs an AI model on your Mac so Soll
          can clean up transcripts and run Skills. Everything stays local — no
          cloud account, no data leaves your computer.
        </>}
        primary={
          <PrimaryButton onClick={() => invoke("install_ollama_via_terminal", { shape: "app" })}>
            Install Ollama
          </PrimaryButton>
        }
        secondary={
          <>
            <SecondaryLink onClick={() => invoke("install_ollama_via_terminal", { shape: "cli" })}>
              Or install the command-line tool instead
            </SecondaryLink>
            <SecondaryLink onClick={onContinue}>
              Skip — Soll dictates fine without AI cleanup
            </SecondaryLink>
          </>
        }
      />
    );
  }

  // ── Ollama installed but not running ─────────────────────────────────────
  return (
    <ConversationalStep
      header={subSteps}
      icon={ICONS.ollama}
      title="Start Ollama"
      subtitle={status.ollama_app_installed
        ? <>The menu-bar Ollama app is installed at <code>/Applications/Ollama.app</code> but isn&apos;t running. Soll will launch it and detect the server within 2 seconds.</>
        : <>The Ollama CLI is installed at <code>/opt/homebrew/bin/ollama</code> but the server isn&apos;t running. Soll will run <code>ollama serve</code> in the background.</>}
      primary={
        <PrimaryButton onClick={() => invoke("open_ollama")}>
          Start Ollama
        </PrimaryButton>
      }
      secondary={
        <SecondaryLink onClick={onContinue}>
          Skip — I&apos;ll set this up later
        </SecondaryLink>
      }
    />
  );
}

function Step5Dictation({ status }: StepProps) {
  // `onContinue` deliberately unused — Step 5 never renders an in-step
  // primary CTA. The footer's "All Done ✓" handles closing the wizard.
  // The "ready" check is intentionally broader than `has_dictated`:
  // permissions can be revoked between sessions (System Settings has
  // its own UI for that), or the user can land on Step 5 with a
  // previous `has_dictated=true` flag stuck in settings while their
  // mic / accessibility grants have lapsed. Showing "You're all set"
  // in that state — as the earlier version did — was misleading.
  const blockers: string[] = [];
  if (!status.model_cached) blockers.push("Speech model isn't downloaded (Step 1)");
  if (status.mic_permission !== "granted") blockers.push("Microphone access (Step 2)");
  if (!status.accessibility) blockers.push("Accessibility access (Step 3)");
  const truly_ready = blockers.length === 0;

  // ── Success: dictation works AND every prerequisite still holds.
  //
  // No primary CTA here — the footer's "All Done ✓" button already
  // closes the wizard with the same effect, and duplicating it inside
  // the step pushed the textarea past the first fold on a 600px-tall
  // window.
  if (status.has_dictated && truly_ready) {
    return (
      <ConversationalStep
        icon={ICONS.dictation}
        success
        title="You're all set"
        subtitle="Hold ⌃⇧Space anywhere on your Mac to dictate. Reopen this guide from the menu-bar icon whenever you need it."
      >
        <DictationTest status={status} />
      </ConversationalStep>
    );
  }

  // ── Blocked: prerequisites are missing. No textarea — it'd just sit
  //    there saying "Resolve the issues below first." Drop it and surface
  //    the blocker list directly, compactly.
  if (!truly_ready) {
    return (
      <ConversationalStep
        icon={ICONS.dictation}
        title="Almost there"
        subtitle={<>
          A few prerequisites still need to be set up before you can dictate.
          Go back and finish them — then come here to test.
        </>}
      >
        <div className="ob-blocker-compact">
          <strong>Still to do:</strong>
          <ul>{blockers.map((b, i) => <li key={i}>{b}</li>)}</ul>
        </div>
      </ConversationalStep>
    );
  }

  // ── Ready but not yet dictated.
  return (
    <ConversationalStep
      icon={ICONS.dictation}
      title="Try it out"
      subtitle={<>
        Click into the box below, hold <kbd>⌃⇧Space</kbd>, speak naturally, then
        release. Your words appear in the box — and in any app you use Soll in.
      </>}
    >
      <DictationTest status={status} />
    </ConversationalStep>
  );
}

function renderStep(idx: number, props: StepProps): React.ReactNode {
  switch (idx) {
    case 0: return <Step1Whisper {...props} />;
    case 1: return <Step2Mic {...props} />;
    case 2: return <Step3Accessibility {...props} />;
    case 3: return <Step4Ollama {...props} />;
    case 4: return <Step5Dictation {...props} />;
    default: return null;
  }
}

// ── Dot progress ───────────────────────────────────────────────────────────

function StepDots({ steps, current, onDotClick }: {
  steps: StepMeta[];
  current: number;
  onDotClick: (i: number) => void;
}) {
  return (
    <div className="ob-dots">
      {steps.map((s, i) => (
        <button
          key={s.id}
          type="button"
          className={
            i === current ? "ob-dot ob-dot--active"
            : s.state === "done" ? "ob-dot ob-dot--done"
            : "ob-dot"
          }
          onClick={() => onDotClick(i)}
          title={s.shortTitle}
        />
      ))}
    </div>
  );
}

// ── Wizard slide wrapper ──────────────────────────────────────────────────
//
// Just provides the slide-in animation around whatever step renderer was
// chosen for `currentStep`. All per-step layout lives inside the renderer
// itself (see Step1Whisper, Step2Mic, etc.).

function WizardSlide({ animDir, children }: {
  animDir: "right" | "left";
  children: React.ReactNode;
}) {
  return (
    <div className={`ob-slide ob-slide--enter-${animDir}`}>
      <div className="ob-slide-inner">
        {children}
      </div>
    </div>
  );
}

// ── Main ───────────────────────────────────────────────────────────────────

export function OnboardingApp() {
  const [status, setStatus]       = useState<OnboardingStatus | null>(null);
  const [models, setModels]       = useState<ModelInfo[]>([]);
  const [ollamaModels, setOllamaModels] = useState<OllamaModelInfo[]>([]);
  const [currentStep, setCurrentStep] = useState(0);
  const [animDir, setAnimDir]     = useState<"right" | "left">("right");
  const [animKey, setAnimKey]     = useState(0);
  // Tracks which step indices the user has actually navigated to during
  // this onboarding session. Without this, the progress bar would jump
  // ahead based purely on prerequisites that happen to already be met
  // (e.g. mic granted from a previous Soll install), which looks wrong
  // when the user hasn't engaged with those steps yet.
  const [visited, setVisited]     = useState<Set<number>>(() => new Set([0]));
  // Tag of the Ollama model currently being pulled, or null. Set when the
  // user clicks a chip, cleared by the polling effect when the chip flips
  // to is_pulled — the backend invoke now returns instantly so we can't
  // rely on its resolution.
  const [pullingOllamaTag, setPullingOllamaTag] = useState<string | null>(null);
  const polling = useRef(false);

  async function fetchStatus() {
    if (polling.current) return;
    polling.current = true;
    try {
      const [s, m, om] = await Promise.all([
        invoke<OnboardingStatus>("onboarding_status"),
        invoke<ModelInfo[]>("models_list"),
        invoke<OllamaModelInfo[]>("ollama_models_list").catch(() => []),
      ]);
      setStatus(s);
      setModels(m);
      setOllamaModels(om);
    } catch (err) {
      console.error("onboarding_status failed:", err);
    } finally {
      polling.current = false;
    }
  }

  useEffect(() => {
    void fetchStatus();
    const id = setInterval(() => void fetchStatus(), 2000);
    return () => clearInterval(id);
  }, []);

  // Clear the local "pulling" flag once polling confirms one of:
  //   • the model finished pulling (success)
  //   • Ollama died (failure — chip would otherwise stay stuck spinning)
  //
  // We *don't* clear on `pull_pct === null` here — that condition is
  // also true in the brief window between clicking pull (which sets
  // pullingOllamaTag) and the backend sending its first progress chunk,
  // so the cleanup would race the kickoff and immediately reset the
  // optimistic flag. Cancellation is handled separately by the picker
  // itself: when the user clicks pause, the picker invokes the
  // `onPullCancel` callback (passed in below) which clears pullingTag
  // synchronously with the cancel command.
  useEffect(() => {
    if (!pullingOllamaTag) return;
    const m = ollamaModels.find(x => x.tag === pullingOllamaTag);
    if (m?.is_pulled) {
      setPullingOllamaTag(null);
    } else if (status && !status.ollama_running) {
      setPullingOllamaTag(null);
    }
  }, [pullingOllamaTag, ollamaModels, status]);

  function goTo(next: number) {
    if (next === currentStep) return;
    setAnimDir(next > currentStep ? "right" : "left");
    setCurrentStep(next);
    setAnimKey(k => k + 1);
    setVisited(prev => {
      if (prev.has(next)) return prev;
      const out = new Set(prev);
      out.add(next);
      return out;
    });
  }

  // "All Done ✓" path — every required step is green. Persists the dismissed
  // flag so the wizard won't auto-open on subsequent launches.
  async function completeAndDismiss() {
    try { await invoke("onboarding_dismiss"); } finally {
      await invoke("close_onboarding_window");
    }
  }

  // "Close" path — at least one required step is unfinished. We do NOT set
  // the dismissed flag; the wizard re-opens on the next launch (and the
  // tray badge stays on) so the user can pick up where they left off.
  async function closeWithoutDismissing() {
    const ok = window.confirm(
      "Setup is incomplete.\n\n" +
      "Some steps are still pending — without them, dictation may not work " +
      "properly. You can reopen this guide anytime from the Soll icon in the " +
      "menu bar.\n\n" +
      "Close anyway?"
    );
    if (!ok) return;
    await invoke("close_onboarding_window");
  }

  if (!status) {
    return <div className="ob-shell"><div className="ob-loading">Loading setup guide…</div></div>;
  }

  const steps       = deriveStepMeta(status, ollamaModels, pullingOllamaTag);
  // A step counts toward "done" only if the user has visited it AND its
  // state is done. Prevents the bar from jumping ahead because of prereqs
  // that happen to already be met from a prior Soll session.
  const doneCount   = steps.filter((s, i) => s.state === "done" && visited.has(i)).length;
  const reqDone     = steps.filter((s, i) => !s.optional && s.state === "done" && visited.has(i)).length;
  const reqTotal    = steps.filter(s => !s.optional).length;
  const allReqDone  = reqDone === reqTotal;
  const pct         = Math.round((doneCount / steps.length) * 100);
  const isFirst     = currentStep === 0;
  const isLast      = currentStep === steps.length - 1;

  const stepProps: StepProps = {
    status,
    models,
    ollamaModels,
    pullingOllamaTag,
    setPullingOllamaTag,
    // Continue/Skip CTAs in the new conversational layout move you forward.
    // On the last step, "Continue" finishes setup (same path as the footer
    // All-Done button).
    onContinue: () => {
      if (isLast) {
        if (allReqDone) void completeAndDismiss();
        else void closeWithoutDismissing();
      } else {
        goTo(currentStep + 1);
      }
    },
  };

  return (
    <div className="ob-shell">
      {/* Header */}
      <div className="ob-header">
        <svg className="ob-logo" viewBox="0 0 28 22" xmlns="http://www.w3.org/2000/svg">
          <rect x="0.5"  y="9"   width="2.5" height="4"  rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="4"    y="7"   width="2.5" height="8"  rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="7.5"  y="3.5" width="3"   height="15" rx="1.5"  fill="currentColor" opacity="0.9"/>
          <rect x="17.5" y="4.5" width="3"   height="13" rx="1.5"  fill="currentColor" opacity="0.9"/>
          <rect x="21.5" y="7"   width="2.5" height="8"  rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="25"   y="9"   width="2.5" height="4"  rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="11.5" y="2.5" width="5"   height="1.5"          fill="#fde047"/>
          <rect x="13.25" y="2.5" width="1.5" height="17"          fill="#fde047"/>
          <rect x="11.5" y="18"  width="5"   height="1.5"          fill="#fde047"/>
        </svg>
        <div>
          <div className="ob-title">Welcome to Soll</div>
          <div className="ob-subtitle">Let's get you set up. Complete the steps below to start dictating.</div>
        </div>
      </div>

      {/* Progress bar */}
      <div className="ob-progress-wrap">
        <div className="ob-progress-bar">
          <div className="ob-progress-fill" style={{ width: `${pct}%` }} />
        </div>
        <span className="ob-progress-label">{doneCount}/{steps.length} steps</span>
      </div>

      {/* Slide */}
      <WizardSlide key={animKey} animDir={animDir}>
        {renderStep(currentStep, stepProps)}
      </WizardSlide>

      {/* Navigation */}
      <div className="ob-nav">
        <button type="button" className="ob-nav-btn" onClick={() => goTo(currentStep - 1)} disabled={isFirst}>
          ← Back
        </button>

        <StepDots steps={steps} current={currentStep} onDotClick={goTo} />

        {isLast ? (
          allReqDone ? (
            <button
              type="button"
              className="ob-nav-btn ob-nav-btn--primary"
              onClick={() => void completeAndDismiss()}
            >
              All Done ✓
            </button>
          ) : (
            <button
              type="button"
              className="ob-nav-btn"
              onClick={() => void closeWithoutDismissing()}
            >
              Close
            </button>
          )
        ) : (
          <button type="button" className="ob-nav-btn ob-nav-btn--next" onClick={() => goTo(currentStep + 1)}>
            Next →
          </button>
        )}
      </div>
    </div>
  );
}
