// INPUT:  React, ReactDOM, App, index.css
// OUTPUT: mounted Hub Desktop React application
// POS:    Browser entrypoint for the Tauri main window.

import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./i18n";
import "./index.css";

const uiLab = import.meta.env.DEV && new URLSearchParams(window.location.search).get("ui-lab") === "1";
const UiLab = import.meta.env.DEV ? lazy(() => import("./ui-lab")) : null;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      {uiLab && UiLab ? <Suspense fallback={null}><UiLab /></Suspense> : <App />}
    </I18nProvider>
  </React.StrictMode>,
);
