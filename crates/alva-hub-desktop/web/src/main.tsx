// INPUT:  React, ReactDOM, App, index.css
// OUTPUT: mounted Hub Desktop React application
// POS:    Browser entrypoint for the Tauri main window.

import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
