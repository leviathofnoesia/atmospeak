import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/tokens.css";
import "./styles/aura.css";
import App, { markOverlayDocument } from "./App";

// Applied before first paint so the overlay never flashes an opaque background.
if (new URLSearchParams(window.location.search).get("view") === "overlay") {
  markOverlayDocument();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
