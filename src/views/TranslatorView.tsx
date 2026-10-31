import { useCallback, useEffect, useState, type ReactNode } from "react";
import { LiveTranslations } from "../components/LiveTranslations";
import { TranslatorSetup } from "../components/TranslatorSetup";
import { useMessages } from "../lib/i18n";
import { setTranslatorEnabled, translatorKeySaved, translatorStatus, type AppError, type TranslatorStatus } from "../lib/ipc";

/** How often the page asks for the status (a few file sizes) while it's open. */
const POLL_MS = 1000;

type View = "translations" | "setup";
const VIEWS: readonly View[] = ["translations", "setup"];

/** The chat translator: its live translations and its setup, as two views. It opens on
 *  Translations once a key is saved or the chat reader is downloaded, on Setup before. */
export function TranslatorView({ status: readerStatus }: { status: ReactNode }) {
  const m = useMessages();
  const t = m.translator;
  const [status, setStatus] = useState<TranslatorStatus | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  // Chosen by the user; until then it follows what's installed.
  const [chosen, setChosen] = useState<View | null>(null);
  // Whether a Gemini key is saved: read on open and after Setup changes it, not polled.
  const [keySaved, setKeySaved] = useState<boolean | null>(null);

  const load = useCallback(async () => {
    const res = await translatorStatus();
    if (res.success) setStatus(res.data);
    else setError(res.error);
  }, []);

  const loadKey = useCallback(async () => {
    const res = await translatorKeySaved();
    if (res.success) setKeySaved(res.data);
    else setError(res.error);
  }, []);

  useEffect(() => {
    void loadKey();
  }, [loadKey]);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), POLL_MS);
    return () => clearInterval(timer);
  }, [load]);

  const downloaded = status?.downloaded === true;
  const enabled = status?.enabled === true;

  const toggle = async () => {
    const res = await setTranslatorEnabled(!enabled);
    setError(res.success ? null : res.error);
    await load();
  };
  const view: View = chosen ?? (status === null || keySaved === null || downloaded || keySaved ? "translations" : "setup");

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          <h1 className="page-title">{t.title}</h1>
          <span className={enabled ? "ok-text translator-state" : "caption translator-state"}>{enabled ? t.on : t.off}</span>
        </div>
        <div className="page-head-actions">
          <div className="segmented" role="tablist" aria-label={t.view}>
            {VIEWS.map((v) => (
              <button key={v} role="tab" aria-selected={v === view} className={v === view ? "current" : ""} onClick={() => setChosen(v)}>
                {t.views[v]}
              </button>
            ))}
          </div>
          <button
            className={enabled ? "btn" : "btn primary"}
            disabled={status === null || (!enabled && !downloaded)}
            title={!enabled && !downloaded ? t.downloadFirst : enabled ? t.stopTitle : t.startTitle}
            onClick={() => void toggle()}
          >
            {enabled ? m.common.turnOff : m.common.turnOn}
          </button>
          {readerStatus}
        </div>
      </section>
      <p className="panel notice" role="note">
        <b>{t.notFinished}</b> {t.notFinishedText}
      </p>
      {error && <p className="panel error">{error.message}</p>}
      {view === "translations" ? (
        <LiveTranslations status={status} keySaved={keySaved} onSetUp={() => setChosen("setup")} onTurnOn={() => void toggle()} />
      ) : (
        <TranslatorSetup status={status} keySaved={keySaved} onError={setError} reload={load} reloadKey={loadKey} />
      )}
    </>
  );
}
