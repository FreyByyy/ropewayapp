import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { Viewer } from "./viewer/Viewer";
import { Indicator } from "./viewer/Indicator";
import { IS_MOBILE } from "./platform";
import "./styles.css";
import "./styles-files.css";
import "./styles-screen.css";
import "./styles-theme.css";
import "./themes/fluent.css";
import "./themes/material.css";
import "./themes/neu.css";
import "./themes/glass.css";
import "./themes/retro.css";
import "./styles-emoji.css";
import "./styles-devices.css";
import "./styles-mobile.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@fontsource/roboto/700.css";
import "@fontsource/nunito/400.css";
import "@fontsource/nunito/600.css";
import "@fontsource/nunito/700.css";
import "@fontsource/nunito/800.css";

if (IS_MOBILE) document.documentElement.dataset.mobile = "true";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {window.location.hash.startsWith("#/viewer") ? (
      <Viewer />
    ) : window.location.hash.startsWith("#/indicator") ? (
      <Indicator />
    ) : (
      <App />
    )}
  </StrictMode>,
);
