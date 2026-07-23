import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "./index.css";
import { applyTheme, readTheme } from "./lib/theme";

// Applied before the first render so a stored dark theme doesn't flash light.
// This runs in module scope rather than an inline <script> in index.html, because the
// production CSP (tauri.conf.json) has no 'unsafe-inline' for scripts.
applyTheme(readTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
