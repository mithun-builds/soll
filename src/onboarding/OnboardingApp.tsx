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
  state: StepState;
  desc: string;
  actionLabel?: string;
  onAction?: () => void;
  extra?: React.ReactNode;
}

// ── Step icons (minimal white SVG line icons) ──────────────────────────────

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
  ollama: (
    <img src={ollamaLogo} className="ob-step-icon-img" alt="Ollama"/>
  ),
  dictation: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>
    </svg>
  ),
  skills: (
    <svg className="ob-step-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
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

function OllamaInstructions() {
  const [open, setOpen] = useState(false);
  return (
    <div className="ob-ollama-wrap">
      <button
        type="button"
        className="ob-ollama-toggle"
        onClick={() => setOpen((o) => !o)}
      >
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

// ── Step derivation ────────────────────────────────────────────────────────

function deriveSteps(s: OnboardingStatus): StepDef[] {
  const modelState: StepState = s.model_cached
    ? "done"
    : s.model_downloading
    ? "in_progress"
    : "pending";

  const micState: StepState =
    s.mic_permission === "granted"
      ? "done"
      : s.mic_permission === "denied"
      ? "denied"
      : "pending";

  const axState: StepState = s.accessibility ? "done" : "pending";
  const ollamaState: StepState = s.ollama_running ? "done" : "pending";
  const dictState: StepState = s.has_dictated ? "done" : "pending";

  return [
    {
      id: "model",
      iconNode: ICONS.model,
      title: "Speech recognition model",
      state: modelState,
      desc: s.model_downloading
        ? `Downloading…${s.model_download_pct != null ? ` ${s.model_download_pct}%` : ""}`
        : "Soll needs a local Whisper model to transcribe your voice. A one-time download that runs entirely on your Mac — nothing leaves your device.",
      actionLabel: modelState === "pending" ? "Download Model" : undefined,
      onAction:
        modelState === "pending"
          ? async () => {
              type M = { id: string; is_active: boolean };
              const list = await invoke<M[]>("models_list");
              const active = list.find((m) => m.is_active);
              if (active) await invoke("model_download", { id: active.id });
            }
          : undefined,
    },
    {
      id: "mic",
      iconNode: ICONS.mic,
      title: "Microphone access",
      state: micState,
      desc:
        micState === "denied"
          ? "Microphone access was denied. Open System Settings and allow Soll to use the microphone."
          : "Soll needs microphone access to record your voice when you hold the shortcut.",
      actionLabel:
        micState === "denied"
          ? "Open Mic Settings"
          : micState === "pending"
          ? "Grant Microphone Access"
          : undefined,
      onAction:
        micState === "denied"
          ? () => invoke("open_privacy_settings", { section: "Privacy_Microphone" })
          : micState === "pending"
          ? () => invoke("request_mic_permission")
          : undefined,
    },
    {
      id: "accessibility",
      iconNode: ICONS.accessibility,
      title: "Accessibility access",
      state: axState,
      desc: "Soll uses Accessibility to paste text into any app. Without it, transcribed text won't be inserted at your cursor.",
      actionLabel: axState !== "done" ? "Open Accessibility Settings" : undefined,
      onAction:
        axState !== "done"
          ? () => invoke("open_privacy_settings", { section: "Privacy_Accessibility" })
          : undefined,
    },
    {
      id: "ollama",
      iconNode: ICONS.ollama,
      title: "Ollama — AI cleanup",
      state: ollamaState,
      desc: "Ollama runs a local AI model to polish your dictation — fixing grammar, punctuation, and capitalisation. Great for longer dictations.",
      extra: ollamaState !== "done" ? <OllamaInstructions /> : undefined,
    },
    {
      id: "dictation",
      iconNode: ICONS.dictation,
      title: "Your first dictation",
      state: dictState,
      desc: "Hold ⌃⇧Space anywhere, speak naturally, then release. Soll transcribes and pastes your words into the focused app.",
    },
  ];
}

// ── Dot progress ───────────────────────────────────────────────────────────

function StepDots({
  steps,
  current,
  onDotClick,
}: {
  steps: StepDef[];
  current: number;
  onDotClick: (i: number) => void;
}) {
  return (
    <div className="ob-dots">
      {steps.map((s, i) => {
        const cls =
          i === current
            ? "ob-dot ob-dot--active"
            : s.state === "done"
            ? "ob-dot ob-dot--done"
            : "ob-dot";
        return (
          <button
            key={s.id}
            type="button"
            className={cls}
            onClick={() => onDotClick(i)}
            title={s.title}
          />
        );
      })}
    </div>
  );
}

// ── Wizard step display ────────────────────────────────────────────────────

function WizardStep({
  step,
  index,
  total,
  animDir,
}: {
  step: StepDef;
  index: number;
  total: number;
  animDir: "right" | "left";
}) {
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

        {step.actionLabel && step.onAction && (
          <button
            type="button"
            className="secondary ob-step-action-btn"
            onClick={step.onAction}
          >
            {step.actionLabel}
          </button>
        )}

        {step.extra && (
          <div className="ob-step-extra">{step.extra}</div>
        )}
      </div>
    </div>
  );
}

// ── Main component ─────────────────────────────────────────────────────────

export function OnboardingApp() {
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [currentStep, setCurrentStep] = useState(0);
  const [animDir, setAnimDir] = useState<"right" | "left">("right");
  const [animKey, setAnimKey] = useState(0);
  const polling = useRef(false);

  async function fetchStatus() {
    if (polling.current) return;
    polling.current = true;
    try {
      const s = await invoke<OnboardingStatus>("onboarding_status");
      setStatus(s);
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

  function goTo(next: number) {
    if (next === currentStep) return;
    setAnimDir(next > currentStep ? "right" : "left");
    setCurrentStep(next);
    setAnimKey((k) => k + 1);
  }

  async function dismiss() {
    try {
      await invoke("onboarding_dismiss");
    } finally {
      await invoke("close_onboarding_window");
    }
  }

  if (!status) {
    return (
      <div className="ob-shell">
        <div className="ob-loading">Loading setup guide…</div>
      </div>
    );
  }

  const steps = deriveSteps(status);
  const totalSteps = steps.length;
  const doneCount = steps.filter((s) => s.state === "done").length;
  const requiredDone = steps.filter((s) => !s.optional && s.state === "done").length;
  const requiredTotal = steps.filter((s) => !s.optional).length;
  const allRequiredDone = requiredDone === requiredTotal;
  const pct = Math.round((doneCount / totalSteps) * 100);

  const isFirst = currentStep === 0;
  const isLast = currentStep === totalSteps - 1;
  const showDone = isLast && allRequiredDone;

  return (
    <div className="ob-shell">
      {/* ── Header ──────────────────────────────────────────────── */}
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
          <div className="ob-subtitle">
            Let's get you set up. Complete the steps below to start dictating.
          </div>
        </div>
      </div>

      {/* ── Progress bar ────────────────────────────────────────── */}
      <div className="ob-progress-wrap">
        <div className="ob-progress-bar">
          <div className="ob-progress-fill" style={{ width: `${pct}%` }} />
        </div>
        <span className="ob-progress-label">{doneCount}/{totalSteps} steps</span>
      </div>

      {/* ── Wizard slide ─────────────────────────────────────────── */}
      <WizardStep
        key={animKey}
        step={steps[currentStep]}
        index={currentStep}
        total={totalSteps}
        animDir={animDir}
      />

      {/* ── Navigation ───────────────────────────────────────────── */}
      <div className="ob-nav">
        <button
          type="button"
          className="ob-nav-btn"
          onClick={() => goTo(currentStep - 1)}
          disabled={isFirst}
        >
          ← Back
        </button>

        <div className="ob-nav-center">
          <StepDots steps={steps} current={currentStep} onDotClick={goTo} />
        </div>

        {isLast ? (
          <button
            type="button"
            className={`ob-nav-btn ob-nav-btn--finish ${showDone ? "ob-nav-btn--primary" : ""}`}
            onClick={() => void dismiss()}
          >
            {showDone ? "All Done ✓" : "Close"}
          </button>
        ) : (
          <button
            type="button"
            className="ob-nav-btn ob-nav-btn--next"
            onClick={() => goTo(currentStep + 1)}
          >
            Next →
          </button>
        )}
      </div>
    </div>
  );
}
