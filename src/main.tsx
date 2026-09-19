import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/archivo/400.css";
import "@fontsource/archivo/500.css";
import "@fontsource/archivo/600.css";
import "@fontsource/archivo/700.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import App from "./App";

const rootElement = document.getElementById("root");
if (rootElement === null) {
  throw new Error("root element not found");
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
