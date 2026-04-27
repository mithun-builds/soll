import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ollamaLogo from "../assets/ollama.png";
import "./onboarding.css";

// ── Types ──────────────────────────────────────────────────────────────────

interface OnboardingStatus {
  model_cached: boolean;
  model_downloading: boolean;
  model_download_pct: number | null;
  mic_permission: "granted" | "denied" | "unknown";
  accessibility: boolean;
  ollama_running: boolean;
  ollama_active_model_pulled: boolean;
  ollama_installed: boolean;
  has_dictated: boolean;
  has_skills: boolean;
  dismissed: boolean;
}

type StepState = "done" | "in_progress" | "denied" | "pending";

interface StepDef {
  id: string;
  iconNode: React.ReactNode;
  title: string;
  optional?: boolean;
  /// Force the toggle to render even when there are no handlers (visual indicator only).
  alwaysShowToggle?: boolean;
  state: StepState;
  desc: string;
  onToggleOn?: () => void | Promise<void>;
  onToggleOff?: () => void | Promise<void>;
  /// Small italic note shown only while the toggle is ON. Used for the macOS
  /// "revoke must be done in System Settings" caveat.
  onNote?: string;
  /// Extra UI below the toggle — install instructions, delete buttons, etc.
  extra?: React.ReactNode;
}

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

function cssState(state: StepState) {
  return state === "in_progress" ? "in-progress" : state;
}

// ── Sub-components ─────────────────────────────────────────────────────────

function StatusBadge({ state }: { state: StepState }) {
  const label: Record<StepState, string> = {
    done: "Done ✓",
    in_progress: "In Progress…",
    denied: "Access Denied",
    pending: "Pending",
  };
  return (
    <span className={`ob-badge ob-badge--${cssState(state)}`}>
      {label[state]}
    </span>
  );
}

interface ModelInfo {
  id: string;
  label: string;
  size: string;
  is_cached: boolean;
  is_active: boolean;
  is_downloading: boolean;
  is_recommended: boolean;
}

function ModelPicker({ models }: { models: ModelInfo[] }) {
  // Multiple models can be cached. The toggle on a chip reflects "this is
  // the active speech model" — i.e. cached AND active. Click rules:
  //   • Already active+cached → no-op (use Delete button below to remove).
  //   • Cached but inactive → switch to it (no download).
  //   • Not cached → start download; becomes active when finished.
  //   • Downloading → cancel.
  async function handleClick(m: ModelInfo) {
    if (m.is_downloading) {
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
    <div className="ob-model-picker">
      {models.map(m => {
        const on = (m.is_cached && m.is_active) || m.is_downloading;
        // Disable only the chip that's already active+cached — clicking it
        // would be a no-op anyway. Every other chip stays clickable so the
        // user can switch freely or download an additional one.
        const disabled = m.is_cached && m.is_active;
        const status =
          m.is_downloading ? "Downloading…"
          : m.is_cached && !m.is_active ? "Cached · click to use"
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
              {m.is_recommended && (
                <div className="ob-model-card-rec">★ Recommended</div>
              )}
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
}

function OllamaModelPicker({ models, ollamaRunning, pullingTag, onPullStart }: {
  models: OllamaModelInfo[];
  ollamaRunning: boolean;
  pullingTag: string | null;
  onPullStart: (tag: string) => void;
}) {
  // Radio-button semantics: exactly one chip is "on" at a time — the active
  // model that Soll uses for cleanup. `is_pulled` is shown as a separate
  // status badge so the user can see what's already on disk without it
  // overriding the active selection.
  async function handleClickChip(m: OllamaModelInfo) {
    if (!ollamaRunning) return;
    if (pullingTag) return; // wait for any in-flight pull to finish
    if (m.is_active && m.is_pulled) return; // already the default — no-op
    await invoke("ollama_model_set", { tag: m.tag });
    if (!m.is_pulled) {
      onPullStart(m.tag);
      void invoke("ollama_pull_active");
    }
  }

  return (
    <div className="ob-model-picker">
      {models.map(m => {
        const isPulling = pullingTag === m.tag;
        const on = (m.is_active && m.is_pulled) || isPulling;
        const disabled = !ollamaRunning || (pullingTag !== null && !isPulling);
        const recommended = m.tag === "llama3.2:3b";
        const statusLabel = isPulling
          ? "Pulling…"
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
              {recommended && (
                <div className="ob-model-card-rec">★ Recommended</div>
              )}
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
        rows={4}
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

function ActionButton({ onClick, danger, children }: {
  onClick: () => void | Promise<void>;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={`ob-action-btn${danger ? " ob-action-btn--danger" : ""}`}
      onClick={() => { void onClick(); }}
    >
      {children}
    </button>
  );
}

function OllamaInstructions() {
  const [open, setOpen] = useState(false);
  return (
    <div className="ob-ollama-wrap">
      <button type="button" className="ob-ollama-toggle" onClick={() => setOpen(o => !o)}>
        {open ? "▾ Hide install instructions" : "▸ How to install Ollama"}
      </button>
      {open && (
        <div className="ob-ollama-instructions">
          <p className="subtle">1. Install via Homebrew:</p>
          <code className="ob-code-block">brew install ollama</code>
          <p className="subtle">2. Start the server:</p>
          <code className="ob-code-block">ollama serve</code>
          <p className="subtle">3. Pull a model:</p>
          <code className="ob-code-block">ollama pull llama3.2:3b</code>
        </div>
      )}
    </div>
  );
}

// ── Toggle ─────────────────────────────────────────────────────────────────

function Toggle({ on, disabled, onEnable, onDisable }: {
  on: boolean;
  disabled?: boolean;
  onEnable?: () => void;
  onDisable?: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      disabled={disabled}
      className={`ob-toggle ${on ? "ob-toggle--on" : ""}`}
      onClick={() => on ? onDisable?.() : onEnable?.()}
    />
  );
}

// ── Step derivation ────────────────────────────────────────────────────────

interface DeriveOpts {
  pullingOllamaTag: string | null;
  setPullingOllamaTag: (v: string | null) => void;
  models: ModelInfo[];
  ollamaModels: OllamaModelInfo[];
}

function deriveSteps(s: OnboardingStatus, opts: DeriveOpts): StepDef[] {
  const modelState: StepState = s.model_cached ? "done"
    : s.model_downloading ? "in_progress" : "pending";

  const micState: StepState = s.mic_permission === "granted" ? "done"
    : s.mic_permission === "denied" ? "denied" : "pending";

  const axState: StepState  = s.accessibility ? "done" : "pending";

  // Ollama is "done" once the *active* model (the one Soll will use for
  // cleanup) is pulled. We don't accept "any pulled" — having Qwen sitting
  // on disk doesn't help if Llama is the active default and isn't pulled.
  const activeOllamaModel = opts.ollamaModels.find(m => m.is_active);
  const ollamaState: StepState =
    s.ollama_running && activeOllamaModel?.is_pulled ? "done"
    : opts.pullingOllamaTag ? "in_progress"
    : "pending";

  const dictState: StepState   = s.has_dictated  ? "done" : "pending";

  // ── Step 1: Whisper model ────────────────────────────────────────────────
  const activeModel = opts.models.find(m => m.is_active);
  const downloadingModel = opts.models.find(m => m.is_downloading);
  const cachedModel = opts.models.find(m => m.is_cached);
  // The model the toggle would act on right now: the one downloading (if any),
  // otherwise the cached one (if any), otherwise the active selection.
  const focusModel = downloadingModel ?? cachedModel ?? activeModel;
  const focusLabel = focusModel
    ? `${focusModel.label} (${focusModel.size})`
    : "Small (466 MB)";

  const modelStep: StepDef = {
    id: "model",
    iconNode: ICONS.model,
    title: "Speech recognition model",
    state: modelState,
    desc: s.model_downloading
      ? `Downloading ${focusLabel}…${s.model_download_pct != null ? ` ${s.model_download_pct}%` : ""}`
      : modelState === "done"
      ? `${focusLabel} is ready. You can switch models anytime from Settings.`
      : `Pick the model you want — toggle it on to download. You can change this later in Settings.`,
    extra: <ModelPicker models={opts.models} />,
  };

  // ── Step 2: Microphone ───────────────────────────────────────────────────
  // macOS only shows the request dialog once. After a "denied" state (revoked
  // in Settings, or declined in the dialog), AVCaptureDevice.requestAccess
  // returns instantly without prompting — so we must redirect to Settings.
  const micStep: StepDef = {
    id: "mic",
    iconNode: ICONS.mic,
    title: "Microphone access",
    state: micState,
    desc: micState === "denied"
      ? "Microphone access was previously declined. Toggle on to open System Settings — macOS won't show the dialog a second time."
      : "Soll needs microphone access to record your voice when you hold the shortcut.",
    onToggleOn: micState === "pending"
      ? () => invoke("request_mic_permission")
      : micState === "denied"
      ? () => invoke("open_privacy_settings", { section: "Privacy_Microphone" })
      : undefined,
    onToggleOff: micState === "done"
      ? () => invoke("open_privacy_settings", { section: "Privacy_Microphone" })
      : undefined,
    onNote: micState === "done"
      ? "macOS only allows revoking via System Settings — toggling off opens the right pane."
      : undefined,
  };

  // ── Step 3: Accessibility ────────────────────────────────────────────────
  // AXIsProcessTrusted caches its result for the lifetime of the process —
  // a freshly granted permission won't reflect until Soll restarts. Surface
  // a Restart action whenever this step is still pending.
  const axStep: StepDef = {
    id: "accessibility",
    iconNode: ICONS.accessibility,
    title: "Accessibility access",
    state: axState,
    desc: "Soll uses Accessibility to paste text into any app. Without it, transcribed text won't be inserted at your cursor.",
    onToggleOn: axState !== "done"
      ? () => invoke("request_accessibility_permission")
      : undefined,
    onToggleOff: axState === "done"
      ? () => invoke("open_privacy_settings", { section: "Privacy_Accessibility" })
      : undefined,
    onNote: axState === "done"
      ? "macOS only allows revoking via System Settings — toggling off opens the right pane."
      : undefined,
    extra: axState !== "done" ? (
      <div className="ob-step-extra-stack">
        <p className="ob-toggle-note">
          Already granted in System Settings? macOS caches Accessibility status until restart.
        </p>
        <ActionButton onClick={() => invoke("restart_app")}>
          Restart Soll to apply
        </ActionButton>
      </div>
    ) : undefined,
  };

  // ── Step 4: Ollama (mandatory, model picker) ─────────────────────────────
  // Picker handles all the pull/delete logic; the step itself shows no
  // standalone toggle, mirroring step 1.
  const pullingOllamaTag = opts.pullingOllamaTag;
  const pullingOllamaModel = pullingOllamaTag
    ? opts.ollamaModels.find(m => m.tag === pullingOllamaTag)
    : undefined;
  const ollamaStep: StepDef = {
    id: "ollama",
    iconNode: ICONS.ollama,
    title: "Ollama — AI cleanup",
    state: ollamaState,
    desc: !s.ollama_running
      ? s.ollama_installed
        ? "Ollama is installed but not running. Click below to launch it — Soll detects it within 2 seconds."
        : "Ollama isn't installed. Follow the instructions below, then return — Soll detects it within 2 seconds."
      : pullingOllamaModel
      ? `Pulling ${pullingOllamaModel.display_name} (${pullingOllamaModel.size})… first pull can take 5–10 minutes. Safe to leave this window open.`
      : activeOllamaModel?.is_pulled
      ? `${activeOllamaModel.display_name} is your default. You can switch models anytime from Settings.`
      : "Pick a default model — toggling one selects it (and downloads it if not already on disk). You can change this later in Settings.",
    extra: !s.ollama_running ? (
      <div className="ob-step-extra-stack">
        {s.ollama_installed ? (
          <ActionButton onClick={() => invoke("open_ollama")}>
            Open Ollama
          </ActionButton>
        ) : (
          <OllamaInstructions />
        )}
      </div>
    ) : (
      <div className="ob-step-extra-stack">
        <OllamaModelPicker
          models={opts.ollamaModels}
          ollamaRunning={s.ollama_running}
          pullingTag={opts.pullingOllamaTag}
          onPullStart={(tag) => opts.setPullingOllamaTag(tag)}
        />
        {activeOllamaModel?.is_pulled && (
          <ActionButton
            danger
            onClick={async () => {
              if (!confirm(`Delete ${activeOllamaModel.display_name} from Ollama? You'll need to re-pull it before AI cleanup works again.`)) return;
              await invoke("ollama_delete_active");
            }}
          >
            Delete {activeOllamaModel.display_name}
          </ActionButton>
        )}
      </div>
    ),
  };

  // ── Step 5: First dictation ──────────────────────────────────────────────
  const dictStep: StepDef = {
    id: "dictation",
    iconNode: ICONS.dictation,
    title: "Your first dictation",
    state: dictState,
    desc: dictState === "done"
      ? "Dictation is working. You can keep practising in the box, or finish the setup."
      : "Click into the box below, hold ⌃⇧Space, speak naturally, then release.",
    extra: <DictationTest status={s} />,
  };

  return [modelStep, micStep, axStep, ollamaStep, dictStep];
}

// ── Dot progress ───────────────────────────────────────────────────────────

function StepDots({ steps, current, onDotClick }: {
  steps: StepDef[];
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
          title={s.title}
        />
      ))}
    </div>
  );
}

// ── Wizard slide ───────────────────────────────────────────────────────────

function WizardStep({ step, index, total, animDir }: {
  step: StepDef;
  index: number;
  total: number;
  animDir: "right" | "left";
}) {
  const hasHandlers = !!step.onToggleOn || !!step.onToggleOff;
  const showToggle = step.alwaysShowToggle || hasHandlers;
  const toggleOn = step.state === "done" || step.state === "in_progress";
  // Disabled exactly when clicking would do nothing — i.e. the handler the
  // toggle would invoke at its current position is missing.
  const wantedHandler = toggleOn ? step.onToggleOff : step.onToggleOn;
  const toggleDisabled = !wantedHandler;

  return (
    <div className={`ob-slide ob-slide--enter-${animDir}`}>
      <div className="ob-slide-inner">
        <div className="ob-step-icon-wrap">{step.iconNode}</div>

        <div className="ob-step-meta">
          <span className="ob-step-num">Step {index + 1} of {total}</span>
          {step.optional && <span className="ob-optional-tag">Optional</span>}
          <StatusBadge state={step.state} />
        </div>

        <div className="ob-step-title">{step.title}</div>
        <p className="ob-step-desc">{step.desc}</p>

        {showToggle && (
          <Toggle
            on={toggleOn}
            disabled={toggleDisabled}
            onEnable={step.onToggleOn}
            onDisable={step.onToggleOff}
          />
        )}

        {step.onNote && toggleOn && (
          <p className="ob-toggle-note">{step.onNote}</p>
        )}

        {step.extra && <div className="ob-step-extra">{step.extra}</div>}
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

  // Clear the local "pulling" flag once polling confirms the model is pulled,
  // OR Ollama died (so the chip doesn't stay stuck spinning on a failure).
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

  const steps       = deriveSteps(status, {
    pullingOllamaTag,
    setPullingOllamaTag,
    models,
    ollamaModels,
  });
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
      <WizardStep
        key={animKey}
        step={steps[currentStep]}
        index={currentStep}
        total={steps.length}
        animDir={animDir}
      />

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
