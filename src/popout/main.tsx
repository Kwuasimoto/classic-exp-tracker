import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/titillium-web/400.css";
import "@fontsource/titillium-web/700.css";
import "@fontsource/titillium-web/900.css";
import "./popout.css";
import { Popout } from "./Popout";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Popout />
  </React.StrictMode>,
);
