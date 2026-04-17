import React from "react";
import ReactDOM from "react-dom/client";
import InspectorWindow from "./InspectorWindow";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <InspectorWindow />
  </React.StrictMode>,
);
