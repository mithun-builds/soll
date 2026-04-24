import { useState } from "react";
import { GeneralPane } from "./panes/GeneralPane";
import { ModelsPane } from "./panes/ModelsPane";
import { DictionaryPane } from "./panes/DictionaryPane";
import { SkillsPane, PhrasesPane } from "./panes/SkillsPane";
import { TipsPane } from "./panes/TipsPane";

type Section =
  | "general"
  | "models"
  | "dictionary"
  | "skills"
  | "phrases"
  | "tips";

const NAV: { id: Section; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "◐" },
  { id: "models", label: "Models", icon: "▣" },
  { id: "dictionary", label: "Dictionary", icon: "☱" },
  { id: "skills", label: "Skills", icon: "✦" },
  { id: "phrases", label: "Phrases", icon: "❝" },
  { id: "tips", label: "Tips & Tricks", icon: "★" },
];

export function SettingsApp() {
  const [section, setSection] = useState<Section>("general");

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <svg className="sidebar-logo" viewBox="0 0 28 22" xmlns="http://www.w3.org/2000/svg">
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
          <div className="sidebar-title">Soll</div>
        </div>
        <nav>
          {NAV.map((n) => (
            <button
              key={n.id}
              type="button"
              className={`nav-btn ${section === n.id ? "active" : ""}`}
              onClick={() => setSection(n.id)}
            >
              <span className="nav-icon">{n.icon}</span>
              <span>{n.label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-foot subtle">
          Hold ⌃⇧Space anywhere to dictate.
        </div>
      </aside>
      <main className="pane">
        {section === "general" && <GeneralPane />}
        {section === "models" && <ModelsPane />}
        {section === "dictionary" && <DictionaryPane />}
        {section === "skills" && <SkillsPane />}
        {section === "phrases" && <PhrasesPane />}
        {section === "tips" && <TipsPane />}
      </main>
    </div>
  );
}
