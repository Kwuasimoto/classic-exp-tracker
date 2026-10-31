import { useState, type ReactNode } from "react";
import { formatBytes } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { cancelInstall, dismissInstallError, downloadReader, removeReader, type AppError, type TranslatorStatus } from "../lib/ipc";
import type { Result } from "../lib/result";
import { TranslatorKey } from "./TranslatorKey";

/** The chat translator's Setup view: the user's Gemini key, then the chat reader, which doesn't
 *  come with the app. It's downloaded only from its own Download button, never as a side
 *  effect of something else (translator.rs). */
export function TranslatorSetup({
  status,
  keySaved,
  onError,
  reload,
  reloadKey,
}: {
  status: TranslatorStatus | null;
  keySaved: boolean | null;
  onError: (e: AppError | null) => void;
  reload: () => Promise<void>;
  reloadKey: () => Promise<void>;
}) {
  const m = useMessages();
  const t = m.translator;
  // Removing deletes the download: it asks first.
  const [removing, setRemoving] = useState(false);

  const act = async (action: () => Promise<Result<null, AppError>>) => {
    const res = await action();
    onError(res.success ? null : res.error);
    setRemoving(false);
    await reload();
  };

  const job = status?.job ?? null;
  const running = job !== null && job.error === null;

  const state = (s: TranslatorStatus): ReactNode => {
    if (job !== null && job.error === null) {
      const pct = job.totalBytes === 0 ? 100 : (job.doneBytes / job.totalBytes) * 100;
      return (
        <div className="install-progress">
          <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} aria-label={t.downloadingLabel}>
            <span style={{ width: `${pct}%` }} />
          </div>
          <span className="caption">{t.downloading(formatBytes(job.doneBytes), formatBytes(job.totalBytes))}</span>
        </div>
      );
    }
    if (job !== null && job.error !== null) return <span className="warn-text">{t.stopped(job.error)}</span>;
    if (s.downloaded) return <span className="ok-text">{t.readerDownloaded}</span>;
    return <span className="caption">{t.readerNotDownloaded(formatBytes(s.downloadBytes))}</span>;
  };

  const actions = (s: TranslatorStatus): ReactNode => {
    if (running) return <button className="btn small" onClick={() => void act(cancelInstall)}>{m.common.cancel}</button>;
    if (job !== null && job.error !== null)
      return (
        <>
          <button className="btn small primary" onClick={() => void act(downloadReader)}>{t.retry}</button>
          <button className="btn small" onClick={() => void act(dismissInstallError)}>{m.common.dismiss}</button>
        </>
      );
    if (s.downloaded && removing)
      return (
        <>
          <button className="btn small danger" onClick={() => void act(removeReader)}>{t.remove}</button>
          <button className="btn small" onClick={() => setRemoving(false)}>{m.common.keep}</button>
        </>
      );
    if (s.downloaded) return <button className="btn small" onClick={() => setRemoving(true)}>{t.remove}</button>;
    return <button className="btn small primary" onClick={() => void act(downloadReader)}>{t.download}</button>;
  };

  return (
    <>
      <TranslatorKey saved={keySaved} onError={onError} onChange={reloadKey} />
      <section className="panel">
        <h2>{t.readerTitle}</h2>
        <p className="translator-note">
          {t.readerIntro[0]}<code>src-tauri/src/translator.rs</code>{t.readerIntro[1]}
        </p>
        {status === null ? (
          <p className="caption">{t.checking}</p>
        ) : (
          <div className="reader-row">
            <div className="reader-state">{state(status)}</div>
            <div className="row-actions">{actions(status)}</div>
          </div>
        )}
        {status && (
          <p className="caption translator-note">
            {t.filesGoTo[0]}<code className="path">{status.dir}</code>{t.filesGoTo[1]}
          </p>
        )}
      </section>
    </>
  );
}
