import { useState } from "react";
import "./App.css";
import { CalibrateView } from "./views/CalibrateView";
import { HistoryView } from "./views/HistoryView";
import { LiveView } from "./views/LiveView";

type Tab = "live" | "history" | "calibrate";
const TABS: ReadonlyArray<[Tab, string]> = [
  ["live", "Live"],
  ["history", "History"],
  ["calibrate", "Calibrate"],
];

export default function App() {
  const [tab, setTab] = useState<Tab>("live");
  return (
    <div className="app">
      <nav className="tabs">
        {TABS.map(([id, label]) => (
          <button key={id} className={id === tab ? "active" : ""} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </nav>
      {tab === "live" && <LiveView onCalibrate={() => setTab("calibrate")} />}
      {tab === "history" && <HistoryView />}
      {tab === "calibrate" && <CalibrateView />}
    </div>
  );
}
