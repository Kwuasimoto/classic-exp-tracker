import { useState, type ReactNode } from "react";
import "@fontsource/titillium-web/400.css";
import "@fontsource/titillium-web/700.css";
import "@fontsource/titillium-web/900.css";
import "./App.css";
import { TitleBar } from "./components/TitleBar";
import { BACKDROPS, saveBackdrop, savedBackdrop } from "./lib/backgrounds";
import { statusText } from "./lib/format";
import { useTick } from "./lib/useTick";
import { HistoryView } from "./views/HistoryView";
import { LiveView } from "./views/LiveView";

type Tab = "live" | "history";

const ICON = { fill: "none", stroke: "currentColor", strokeWidth: 2.2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
const TABS: ReadonlyArray<{ id: Tab; label: string; icon: ReactNode }> = [
  { id: "live", label: "Live", icon: <><polyline points="3 17 9 11 13 15 21 7" /><polyline points="15 7 21 7 21 13" /></> },
  { id: "history", label: "History", icon: <><circle cx="12" cy="12" r="9" /><polyline points="12 7 12 12 15 14" /></> },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("live");
  const [backdrop, setBackdrop] = useState(savedBackdrop);
  const { tick, error } = useTick();
  const status = tick?.status ?? null;

  const chooseBackdrop = (id: string) => {
    const next = BACKDROPS.find((b) => b.id === id);
    if (!next) return;
    setBackdrop(next);
    saveBackdrop(id);
  };

  return (
    <div className="window" style={backdrop.css ?? undefined}>
      <TitleBar />
      <div className="shell">
        <nav className="nav" aria-label="Screens">
          {TABS.map((t) => (
            <button key={t.id} className={t.id === tab ? "nav-link current" : "nav-link"} aria-current={t.id === tab ? "page" : undefined} onClick={() => setTab(t.id)}>
              <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true" {...ICON}>{t.icon}</svg>
              {t.label}
            </button>
          ))}
          <div className="nav-footer">
            <label className="picker">
              <span className="label">Background</span>
              <select value={backdrop.id} onChange={(e) => chooseBackdrop(e.target.value)}>
                {BACKDROPS.map((b) => <option key={b.id} value={b.id}>{b.label}</option>)}
              </select>
            </label>
            <div className={status?.kind === "ok" ? "reader-status ok" : "reader-status"} role="status">
              {error ? error.message : status ? statusText(status) : "Starting…"}
            </div>
          </div>
        </nav>
        <main className="content">
          {tab === "live" && <LiveView tick={tick} />}
          {tab === "history" && <HistoryView />}
        </main>
      </div>
    </div>
  );
}
