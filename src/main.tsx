import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import UsageWidget from "./UsageWidget";
import "./styles.css";

const isUsageWindow = new URLSearchParams(window.location.search).get("window") === "usage";
document.body.classList.toggle("widget-body", isUsageWindow);

createRoot(document.getElementById("root")!).render(
  <StrictMode>{isUsageWindow ? <UsageWidget /> : <App />}</StrictMode>,
);
