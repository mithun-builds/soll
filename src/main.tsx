import React from "react";
import ReactDOM from "react-dom/client";
import { SettingsApp } from "./settings/SettingsApp";
import { OverlayApp } from "./overlay/OverlayApp";
import "./styles.css";

const params = new URLSearchParams(window.location.search);
const view = params.get("view");

// Mark the root element so CSS can make the overlay background transparent.
if (view === "overlay") {
  document.documentElement.classList.add("overlay-view");
}

function Root() {
  if (view === "overlay") {
    return <OverlayApp />;
  }
  // All admin UI lives in the unified Settings window.
  // Legacy ?view=dictionary / ?view=legend are redirected for backwards
  // compatibility with any shortcut someone saved from an earlier build.
  if (
    view === "settings" ||
    view === "dictionary" ||
    view === "legend"
  ) {
    return <SettingsApp />;
  }
  return (
    <div className="placeholder">
      <h1>Soll</h1>
      <p>Use the tray icon to open Settings or quit.</p>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
