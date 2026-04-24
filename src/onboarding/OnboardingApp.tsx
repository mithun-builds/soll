import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  icon: string;
  title: string;
  optional?: boolean;
  state: StepState;
  desc: string;
  actionLabel?: string;
  onAction?: () => void;
  extra?: React.ReactNode;
}

// ── Mock (simulate brand-new user) ────────────────────────────────────────

const MOCK_STATUS: OnboardingStatus = {
  model_cached: false,
  model_downloading: false,
  model_download_pct: null,
  mic_permission: "unknown",
  accessibility: false,
  ollama_running: false,
  has_dictated: false,
  has_skills: false,
  dismissed: false,
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
  const skillsState: StepState = s.has_skills ? "done" : "pending";

  return [
    {
      id: "model",
      icon: "◈",
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
      icon: "🎤",
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
        micState !== "done"
          ? () => invoke("open_privacy_settings", { section: "Privacy_Microphone" })
          : undefined,
    },
    {
      id: "accessibility",
      icon: "⌨️",
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
      icon: "🤖",
      title: "Ollama — AI cleanup",
      optional: true,
      state: ollamaState,
      desc: "Ollama runs a local AI model to polish your dictation — fixing grammar, punctuation, and capitalisation. Optional, but great for longer dictations.",
      extra: ollamaState !== "done" ? <OllamaInstructions /> : undefined,
    },
    {
      id: "dictation",
      icon: "✍️",
      title: "Your first dictation",
      state: dictState,
      desc: "Hold ⌃⇧Space anywhere, speak naturally, then release. Soll transcribes and pastes your words into the focused app.",
    },
    {
      id: "skills",
      icon: "⚡",
      title: "Create a skill",
      optional: true,
      state: skillsState,
      desc: "Skills let you trigger AI actions by voice — write a reply, summarise a page, translate text, and more. Create your first skill in Settings.",
      actionLabel: skillsState !== "done" ? "Open Settings" : undefined,
      onAction:
        skillsState !== "done"
          ? () => invoke("open_settings_window_cmd")
          : undefined,
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
        <div className="ob-step-icon">{step.icon}</div>

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
  const [mock, setMock] = useState(false);
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

  const displayed = mock ? MOCK_STATUS : status;
  const steps = deriveSteps(displayed);
  const totalSteps = steps.length;
  const doneCount = steps.filter((s) => s.state === "done").length;
  const requiredDone = steps.filter((s) => !s.optional && s.state === "done").length;
  const requiredTotal = steps.filter((s) => !s.optional).length;
  const allRequiredDone = requiredDone === requiredTotal;
  const pct = Math.round((doneCount / totalSteps) * 100);

  const isFirst = currentStep === 0;
  const isLast = currentStep === totalSteps - 1;
  const showDone = isLast && allRequiredDone && !mock;

  return (
    <div className="ob-shell">
      {/* ── Header ──────────────────────────────────────────────── */}
      <div className="ob-header">
        <svg className="ob-logo" viewBox="0 0 22 22" xmlns="http://www.w3.org/2000/svg">
          <rect x="0.5"   y="9"   width="2"   height="4"  rx="1"    fill="currentColor" opacity="0.9"/>
          <rect x="3.5"   y="7"   width="2"   height="8"  rx="1"    fill="currentColor" opacity="0.9"/>
          <rect x="6.5"   y="3.5" width="2.5" height="15" rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="14"    y="4.5" width="2.5" height="13" rx="1.25" fill="currentColor" opacity="0.9"/>
          <rect x="17.5"  y="7"   width="2"   height="8"  rx="1"    fill="currentColor" opacity="0.9"/>
          <rect x="20.5"  y="9"   width="1.5" height="4"  rx="0.75" fill="currentColor" opacity="0.9"/>
          <rect x="9.5"   y="2.5" width="4"   height="1.5"          fill="#fde047"/>
          <rect x="10.75" y="2.5" width="1.5" height="17"           fill="#fde047"/>
          <rect x="9.5"   y="18"  width="4"   height="1.5"          fill="#fde047"/>
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
          <button
            type="button"
            className="ob-simulate-btn"
            onClick={() => setMock((m) => !m)}
          >
            {mock ? "← Live status" : "Preview as new user"}
          </button>
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
