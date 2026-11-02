import { useEffect, useRef, useState, type ReactNode } from "react";
import "@fontsource/titillium-web/400.css";
import "@fontsource/titillium-web/700.css";
import "@fontsource/titillium-web/900.css";
import "./App.css";
import { LanguagePicker } from "./components/LanguagePicker";
import { PinMenu } from "./components/PinMenu";
import { ReaderStatus } from "./components/ReaderStatus";
import { TitleBar } from "./components/TitleBar";
import { useMessages } from "./lib/i18n";
import { onPopoutClosed, setPopout, type Popout } from "./lib/ipc";
import { saveLayout, savedLayout, type Layout } from "./lib/layout";
import { savePopoutOpen, savedPopoutOpen } from "./lib/popout";
import { compressedSize, currentSize, resizeWindow, saveExpandedSize, savedExpandedSize, type WindowSize } from "./lib/windowSize";
import { useTick } from "./lib/useTick";
import { HistoryView } from "./views/HistoryView";
import { LiveView } from "./views/LiveView";
import { MesoView } from "./views/MesoView";
import { TranslatorView } from "./views/TranslatorView";

type Tab = "live" | "mesos" | "history" | "translator";

const ICON = { fill: "none", stroke: "currentColor", strokeWidth: 2.2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
const TABS: ReadonlyArray<{ id: Tab; icon: ReactNode }> = [
  { id: "live", icon: <><polyline points="3 17 9 11 13 15 21 7" /><polyline points="15 7 21 7 21 13" /></> },
  { id: "mesos", icon: <><circle cx="12" cy="12" r="9" /><path d="M15 9.5a3 3 0 0 0-3-1.5c-1.7 0-3 .9-3 2s1.3 1.7 3 2 3 .9 3 2-1.3 2-3 2a3 3 0 0 1-3-1.5" /><line x1="12" y1="6" x2="12" y2="18" /></> },
  { id: "history", icon: <><circle cx="12" cy="12" r="9" /><polyline points="12 7 12 12 15 14" /></> },
  { id: "translator", icon: <><path d="M21 15a2 2 0 0 1-2 2H8l-5 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M8 8h6M11 7v1M9.5 8c0 2 1.5 3.5 3.5 4M12.5 8c-.5 2-2 3.5-4 4" /></> },
];
// Arrows pointing in (compress) and out (expand).
const COMPRESS = <><polyline points="4 14 10 14 10 20" /><polyline points="20 10 14 10 14 4" /><line x1="14" y1="10" x2="21" y2="3" /><line x1="3" y1="21" x2="10" y2="14" /></>;
const EXPAND = <><polyline points="15 3 21 3 21 9" /><polyline points="9 21 3 21 3 15" /><line x1="21" y1="3" x2="14" y2="10" /><line x1="3" y1="21" x2="10" y2="14" /></>;

export default function App() {
  const m = useMessages();
  const [tab, setTab] = useState<Tab>("live");
  const [layout, setLayout] = useState<Layout>(savedLayout);
  const compressed = layout === "compressed";
  const { tick, error } = useTick();
  const root = useRef<HTMLDivElement>(null);
  const [windowError, setWindowError] = useState<string | null>(null);
  const [popouts, setPopouts] = useState<Record<Popout, boolean>>({ rates: false, chat: false });
  const status = <ReaderStatus status={tick?.status ?? null} error={error} />;

  const resize = (size: WindowSize) =>
    void resizeWindow(size).then((res) => setWindowError(res.success ? null : `resize: ${res.error}`));

  const toggleLayout = () => {
    const next: Layout = compressed ? "expanded" : "compressed";
    if (next === "compressed") saveExpandedSize(currentSize());
    else resize(savedExpandedSize());
    setLayout(next);
    saveLayout(next);
  };

  const openPopout = (popout: Popout, open: boolean) =>
    void setPopout(popout, open).then((res) => {
      if (!res.success) return setWindowError(`popout: ${res.error.message}`);
      setPopouts((now) => ({ ...now, [popout]: open }));
      savePopoutOpen(popout, open);
    });
  const togglePopout = (popout: Popout) => openPopout(popout, !popouts[popout]);

  // Each popout reopens with the app if it was open; its own ✕ closes it.
  useEffect(() => {
    for (const popout of ["rates", "chat"] as const) {
      if (savedPopoutOpen(popout)) openPopout(popout, true);
    }
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void onPopoutClosed((popout) => {
      setPopouts((now) => ({ ...now, [popout]: false }));
      savePopoutOpen(popout, false);
    }).then((res) => {
      if (!res.success) return setWindowError(`popout: ${res.error.message}`);
      if (disposed) res.data();
      else unlisten = res.data;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Compressed: narrow the window to the rail and the ledger, keeping the expanded height.
  useEffect(() => {
    if (!compressed || !root.current) return;
    const size = compressedSize(root.current, savedExpandedSize());
    if (!size.success) return setWindowError(`resize: ${size.error}`);
    resize(size.data);
  }, [compressed]);

  return (
    <div ref={root} className={compressed ? "window compressed" : "window"}>
      <TitleBar error={windowError} />
      <div className="shell">
        <nav className="rail" aria-label={m.app.screens}>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={t.id === tab ? "rail-btn current" : "rail-btn"}
              aria-label={m.app.tabs[t.id]}
              title={m.app.tabs[t.id]}
              aria-current={t.id === tab ? "page" : undefined}
              onClick={() => setTab(t.id)}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true" {...ICON}>{t.icon}</svg>
            </button>
          ))}
          <span style={{ flex: 1 }} />
          <PinMenu open={popouts} onToggle={togglePopout} />
          <button
            className="rail-btn"
            aria-label={m.app.compressed}
            aria-pressed={compressed}
            title={compressed ? m.app.toExpandedTitle : m.app.toCompressedTitle}
            onClick={toggleLayout}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true" {...ICON}>{compressed ? EXPAND : COMPRESS}</svg>
          </button>
          <LanguagePicker />
        </nav>
        <main className="content">
          {tab === "live" && <LiveView tick={tick} status={status} />}
          {tab === "mesos" && <MesoView tick={tick} status={status} />}
          {tab === "history" && <HistoryView status={status} />}
          {tab === "translator" && <TranslatorView status={status} />}
        </main>
      </div>
    </div>
  );
}
