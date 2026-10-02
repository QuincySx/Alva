// INPUT:  React, ReactDOM, App, appearance/i18n providers, index.css
// OUTPUT: mounted Hub Desktop React application
// POS:    Browser entrypoint for the Tauri main window.

import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./i18n";
import { AppearanceProvider } from "./appearance";
import "./index.css";

const uiLab = import.meta.env.DEV && new URLSearchParams(window.location.search).get("ui-lab") === "1";
const UiLab = import.meta.env.DEV ? lazy(() => import("./ui-lab")) : null;
// H19/H21 evidence harness: URL-gated (works in production builds), never
// linked from product surfaces.
const perf = new URLSearchParams(window.location.search).get("perf") === "1";
const PerfHarness = lazy(() => import("./perf/perf-harness"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      {perf ? (
        <Suspense fallback={null}><PerfHarness /></Suspense>
      ) : uiLab && UiLab ? (
        <AppearanceProvider><Suspense fallback={null}><UiLab /></Suspense></AppearanceProvider>
      ) : (
        <App />
      )}
    </I18nProvider>
  </React.StrictMode>,
);
