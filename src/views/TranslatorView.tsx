import { useCallback, useEffect, useState, type ReactNode } from "react";
import { LiveTranslations } from "../components/LiveTranslations";
import { TranslatorSetup } from "../components/TranslatorSetup";
import { setTranslatorEnabled, translatorStatus, type AppError, type TranslatorStatus } from "../lib/ipc";

/** How often the page asks for the status (a few file sizes) while it's open. */
const POLL_MS = 1000;

type View = "translations" | "setup";
const VIEWS: ReadonlyArray<{ id: View; label: string }> = [
  { id: "translations", label: "Translations" },
  { id: "setup", label: "Setup" },
];

/** The chat translator: its live translations and its setup, as two views. It opens on
 *  Translations when a language is installed, on Setup when none is. */
export function TranslatorView({ status: readerStatus }: { status: ReactNode }) {
  const [status, setStatus] = useState<TranslatorStatus | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  // Chosen by the user; until then it follows what's installed.
  const [chosen, setChosen] = useState<View | null>(null);

  const load = useCallback(async () => {
    const res = await translatorStatus();
    if (res.success) setStatus(res.data);
    else setError(res.error);
  }, []);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), POLL_MS);
    return () => clearInterval(timer);
  }, [load]);

  const anyInstalled = status?.languages.some((l) => l.installed) ?? false;
  const enabled = status?.enabled === true;

  const toggle = async () => {
    const res = await setTranslatorEnabled(!enabled);
    setError(res.success ? null : res.error);
    await load();
  };
  const view: View = chosen ?? (status === null || anyInstalled ? "translations" : "setup");

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          <span className="page-title">Chat translator</span>
          <span className={enabled ? "ok-text translator-state" : "caption translator-state"}>{enabled ? "On" : "Off"}</span>
        </div>
        <div className="page-head-actions">
          <div className="segmented" role="tablist" aria-label="Chat translator view">
            {VIEWS.map((v) => (
              <button key={v.id} role="tab" aria-selected={v.id === view} className={v.id === view ? "current" : ""} onClick={() => setChosen(v.id)}>
                {v.label}
              </button>
            ))}
          </div>
          <button
            className={enabled ? "btn" : "btn primary"}
            disabled={status === null || (!enabled && !anyInstalled)}
            title={!enabled && !anyInstalled ? "Install a language in Setup first" : enabled ? "Stop reading and translating the chat" : "Read and translate the chat (uses noticeable processing power and over 1 GB of memory)"}
            onClick={() => void toggle()}
          >
            {enabled ? "Turn off" : "Turn on"}
          </button>
          {readerStatus}
        </div>
      </section>
      <p className="panel notice" role="note">
        <b>Not finished yet.</b> Reading and translating the chat are expected in the next release. You can already install languages here, so they're ready when it arrives.
      </p>
      {error && <p className="panel error">{error.message}</p>}
      {view === "translations" ? (
        <LiveTranslations status={status} onSetUp={() => setChosen("setup")} onTurnOn={() => void toggle()} />
      ) : (
        <TranslatorSetup status={status} onError={setError} reload={load} />
      )}
    </>
  );
}
