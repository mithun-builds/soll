import React from "react";
import ReactDOM from "react-dom/client";
import { DictionaryApp } from "./dictionary/DictionaryApp";
import "./styles.css";

const params = new URLSearchParams(window.location.search);
const view = params.get("view");

function Root() {
  if (view === "dictionary") return <DictionaryApp />;
  // Svara is tray-first. No other views exist yet; if someone lands on
  // the default URL, show a minimal "nothing here" placeholder.
  return (
    <div className="placeholder">
      <h1>Svara</h1>
      <p>Use the menu-bar icon to open the dictionary or quit.</p>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
