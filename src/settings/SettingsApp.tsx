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
        <div className="sidebar-title">Soll</div>
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
