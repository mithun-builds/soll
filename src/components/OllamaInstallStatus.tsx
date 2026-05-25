import { useState } from "react";
import "./OllamaInstallStatus.css";

/// Shared "is Ollama installed?" display, used in two places:
///
///   • Setup Guide Step 4 — pills go inline with the sub-step title
///     (so the row 1 mark centers against title + pills only), and the
///     disclosure renders below in a second grid row.
///
///   • Settings → Models pane — pills and disclosure stack together
///     as a single block via the convenience `<OllamaInstallStatus />`
///     wrapper, which is unchanged.
///
/// The components are exported individually so callers can lay them out
/// however they need without forking the markup or the verify-tip copy.

export interface OllamaInstallProps {
  appInstalled: boolean;
  cliInstalled: boolean;
  /** When true the disclosure expands and the verify-or-install tips
   *  are visible from the start. Used by Settings → Models where the
   *  panel is already inside a settings pane and there's no harm in
   *  the tips being visible without an extra click. */
  defaultOpen?: boolean;
}

/// Pills + disclosure, stacked. Used as-is in Settings → Models pane.
export function OllamaInstallStatus({
  appInstalled,
  cliInstalled,
  defaultOpen = false,
}: OllamaInstallProps) {
  return (
    <div className="oi-status">
      <OllamaInstallPills appInstalled={appInstalled} cliInstalled={cliInstalled} />
      <OllamaInstallTipsDisclosure
        appInstalled={appInstalled}
        cliInstalled={cliInstalled}
        defaultOpen={defaultOpen}
      />
    </div>
  );
}

/// Just the two install-shape pills (App + CLI). Caller is responsible
/// for layout context. The "App" pill refers to the menu-bar Ollama
/// app at `/Applications/Ollama.app`; "Menu-bar app" was used previously
/// but the longer label forced wrapping inside the narrow Step-4
/// sub-step columns.
export function OllamaInstallPills({
  appInstalled,
  cliInstalled,
}: Pick<OllamaInstallProps, "appInstalled" | "cliInstalled">) {
  return (
    <div className="oi-pills">
      <OiPill present={appInstalled} label="App" />
      <OiPill present={cliInstalled} label="CLI" />
    </div>
  );
}

/// Just the verify-tip content (two paragraphs, one per install shape).
/// No state, no toggle — exported separately so the onboarding
/// substep panel can host its own collapse/expand state and place the
/// trigger inline with the substep title instead of below the pills.
export function OllamaInstallVerifyTipsContent({
  appInstalled,
  cliInstalled,
}: Pick<OllamaInstallProps, "appInstalled" | "cliInstalled">) {
  return (
    <div className="oi-tips">
      <p>
        <strong>App:</strong>{" "}
        {appInstalled
          ? <>Find <code>Ollama</code> at <code>/Applications/Ollama.app</code>, or look for the llama icon in your menu bar when it&apos;s running.</>
          : <>Would install to <code>/Applications/Ollama.app</code>. Run <code>brew install --cask ollama</code> in Terminal.</>}
      </p>
      <p>
        <strong>CLI:</strong>{" "}
        {cliInstalled
          ? <>Run <code>which ollama</code> or <code>ollama --version</code> in Terminal.</>
          : <>Would install to <code>/opt/homebrew/bin/ollama</code>. Run <code>brew install ollama</code> in Terminal.</>}
      </p>
    </div>
  );
}

/// Click-to-expand "How to verify or install" disclosure. Has its own
/// collapsed/open state and renders `OllamaInstallVerifyTipsContent`
/// when opened. Used directly in Settings → Models; the onboarding
/// substep panel uses the content component above with its own state
/// because it places the trigger inline with the substep title.
export function OllamaInstallTipsDisclosure({
  appInstalled,
  cliInstalled,
  defaultOpen = false,
}: OllamaInstallProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="oi-disclose-wrap">
      <button
        type="button"
        className="oi-disclose"
        onClick={() => setOpen(o => !o)}
        aria-expanded={open}
      >
        {open ? "▾" : "▸"} How to verify or install
      </button>
      {open && (
        <OllamaInstallVerifyTipsContent
          appInstalled={appInstalled}
          cliInstalled={cliInstalled}
        />
      )}
    </div>
  );
}

function OiPill({ present, label }: { present: boolean; label: string }) {
  return (
    <span className={`oi-pill ${present ? "oi-pill--present" : "oi-pill--absent"}`}>
      <span className="oi-pill-mark">{present ? "✓" : "—"}</span>
      {label}
    </span>
  );
}
