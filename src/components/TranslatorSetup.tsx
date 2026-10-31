import { useState, type ReactNode } from "react";
import { formatBytes } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { cancelInstall, dismissInstallError, installLanguage, removeLanguage, type AppError, type TranslatorStatus } from "../lib/ipc";
import type { Result } from "../lib/result";

type Language = TranslatorStatus["languages"][number];

/** The chat translator's Setup view: install or remove a language's files, which don't come
 *  with the app, and every file that can be downloaded (translator.rs). */
export function TranslatorSetup({ status, onError, reload }: { status: TranslatorStatus | null; onError: (e: AppError | null) => void; reload: () => Promise<void> }) {
  const m = useMessages();
  const t = m.translator;
  // Removing deletes a large download: it asks first.
  const [removing, setRemoving] = useState<string | null>(null);

  const act = async (action: () => Promise<Result<null, AppError>>) => {
    const res = await action();
    onError(res.success ? null : res.error);
    setRemoving(null);
    await reload();
  };

  const job = status?.job ?? null;
  const running = job !== null && job.error === null;
  const noneInstalled = status !== null && status.languages.every((l) => !l.installed);
  const name = (id: string) => t.languages[id] ?? id;
  const others = (l: Language) => l.sharesWith.map(name).join(", ");

  const state = (l: Language): ReactNode => {
    if (job?.language === l.id && job.error === null) {
      const pct = job.totalBytes === 0 ? 100 : (job.doneBytes / job.totalBytes) * 100;
      return (
        <div className="install-progress">
          <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} aria-label={t.downloadingLabel(name(l.id))}>
            <span style={{ width: `${pct}%` }} />
          </div>
          <span className="caption">{t.downloading(formatBytes(job.doneBytes), formatBytes(job.totalBytes))}</span>
        </div>
      );
    }
    if (job?.language === l.id && job.error !== null) return <span className="warn-text">{t.stopped(job.error)}</span>;
    if (l.installed) return <span className="ok-text">{l.sharesWith.length > 0 ? t.installedWith(others(l)) : t.installed}</span>;
    const size = formatBytes(l.downloadBytes);
    return <span className="caption">{l.sharesWith.length > 0 ? t.notInstalledWith(size, others(l)) : t.notInstalled(size)}</span>;
  };

  const actions = (l: Language): ReactNode => {
    if (job?.language === l.id && job.error === null) return <button className="btn small" onClick={() => void act(cancelInstall)}>{m.common.cancel}</button>;
    if (job?.language === l.id && job.error !== null)
      return (
        <>
          <button className="btn small primary" onClick={() => void act(() => installLanguage(l.id))}>{t.retry}</button>
          <button className="btn small" onClick={() => void act(dismissInstallError)}>{m.common.dismiss}</button>
        </>
      );
    if (l.installed && removing === l.id)
      return (
        <>
          <button className="btn small danger" onClick={() => void act(() => removeLanguage(l.id))}>
            {l.sharesWith.length > 0 ? t.removeWith(others(l)) : t.remove}
          </button>
          <button className="btn small" onClick={() => setRemoving(null)}>{m.common.keep}</button>
        </>
      );
    if (l.installed) return <button className="btn small" disabled={running} onClick={() => setRemoving(l.id)}>{t.remove}</button>;
    return <button className="btn small primary" disabled={running} onClick={() => void act(() => installLanguage(l.id))}>{t.install}</button>;
  };

  return (
    <>
      <section className="panel">
        <h2>{t.languagesTitle}</h2>
        {noneInstalled && !running && (
          <p className="translator-empty">
            <b>{t.noneInstalled}</b> {t.setupEmpty}
          </p>
        )}
        {status === null ? (
          <p className="caption">{t.checking}</p>
        ) : (
          <table className="languages">
            <colgroup>
              <col style={{ width: 190 }} />
              <col />
              <col style={{ width: 210 }} />
            </colgroup>
            <tbody>
              {status.languages.map((l) => (
                <tr key={l.id}>
                  <td>{name(l.id)}</td>
                  <td>{state(l)}</td>
                  <td><div className="row-actions">{actions(l)}</div></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="caption translator-note">{t.setupNote}</p>
      </section>

      {status && (
        <section className="panel">
          <h2>{t.downloadsTitle}</h2>
          <p className="caption translator-note">
            {t.downloadsNote[0]}<code>src-tauri/src/translator.rs</code>{t.downloadsNote[1]} {t.filesGoTo[0]}<code className="path">{status.dir}</code>{t.filesGoTo[1]}
          </p>
          <table className="downloads">
            <colgroup>
              <col style={{ width: 200 }} />
              <col />
              <col style={{ width: 80 }} />
              <col style={{ width: 90 }} />
            </colgroup>
            <thead>
              <tr><th>{t.part}</th><th>{t.fileAndHash}</th><th className="num">{t.size}</th><th>{t.license}</th></tr>
            </thead>
            <tbody>
              {status.files.map((f) => (
                <tr key={f.url}>
                  <td>{t.parts[f.partId] ?? f.part}</td>
                  <td>
                    <span className="cut" title={f.url}>{f.source} · {f.name}</span>
                    <span className="cut caption hash" title={f.sha256}>{f.sha256}</span>
                  </td>
                  <td className="num">{formatBytes(f.size)}</td>
                  <td>{f.license}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </>
  );
}
