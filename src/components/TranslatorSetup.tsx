import { useState, type ReactNode } from "react";
import { formatBytes } from "../lib/format";
import { cancelInstall, dismissInstallError, installLanguage, removeLanguage, type AppError, type TranslatorStatus } from "../lib/ipc";
import type { Result } from "../lib/result";

type Language = TranslatorStatus["languages"][number];

/** The chat translator's Setup view: install or remove a language's files, which don't come
 *  with the app, and every file that can be downloaded (translator.rs). */
export function TranslatorSetup({ status, onError, reload }: { status: TranslatorStatus | null; onError: (e: AppError | null) => void; reload: () => Promise<void> }) {
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

  const state = (l: Language): ReactNode => {
    if (job?.language === l.id && job.error === null) {
      const pct = job.totalBytes === 0 ? 100 : (job.doneBytes / job.totalBytes) * 100;
      return (
        <div className="install-progress">
          <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} aria-label={`Downloading ${l.label}`}>
            <span style={{ width: `${pct}%` }} />
          </div>
          <span className="caption">Downloading {formatBytes(job.doneBytes)} of {formatBytes(job.totalBytes)}</span>
        </div>
      );
    }
    if (job?.language === l.id && job.error !== null) return <span className="warn-text">Stopped: {job.error}</span>;
    if (l.installed) return <span className="ok-text">Installed{l.sharesWith.length > 0 ? `, with ${l.sharesWith.join(", ")} (same files)` : ""}</span>;
    return <span className="caption">Not installed · {formatBytes(l.downloadBytes)} download{l.sharesWith.length > 0 ? `, installs ${l.sharesWith.join(", ")} too` : ""}</span>;
  };

  const actions = (l: Language): ReactNode => {
    if (job?.language === l.id && job.error === null) return <button className="btn small" onClick={() => void act(cancelInstall)}>Cancel</button>;
    if (job?.language === l.id && job.error !== null)
      return (
        <>
          <button className="btn small primary" onClick={() => void act(() => installLanguage(l.id))}>Retry</button>
          <button className="btn small" onClick={() => void act(dismissInstallError)}>Dismiss</button>
        </>
      );
    if (l.installed && removing === l.id)
      return (
        <>
          <button className="btn small danger" onClick={() => void act(() => removeLanguage(l.id))}>
            Remove{l.sharesWith.length > 0 ? ` with ${l.sharesWith.join(", ")}` : ""}
          </button>
          <button className="btn small" onClick={() => setRemoving(null)}>Keep</button>
        </>
      );
    if (l.installed) return <button className="btn small" disabled={running} onClick={() => setRemoving(l.id)}>Remove</button>;
    return <button className="btn small primary" disabled={running} onClick={() => void act(() => installLanguage(l.id))}>Install</button>;
  };

  return (
    <>
      <section className="panel">
        <h2>Languages</h2>
        {noneInstalled && !running && (
          <p className="translator-empty">
            <b>No translator installed.</b> It isn't part of the app's installer: install a language below to download what it needs.
          </p>
        )}
        {status === null ? (
          <p className="caption">Checking what's installed…</p>
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
                  <td>{l.label}</td>
                  <td>{state(l)}</td>
                  <td><div className="row-actions">{actions(l)}</div></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="caption translator-note">
          Each language is read from the chat and translated into English on this PC; nothing you or other players type is sent anywhere. Reading the chat isn't switched on yet: this page installs what it will need.
        </p>
      </section>

      {status && (
        <section className="panel">
          <h2>What gets downloaded</h2>
          <p className="caption translator-note">
            Only when you click Install, from Hugging Face, and nothing else is sent. Each file is pinned to a revision of its repository and kept only if its SHA-256 matches the one below (the same list is in <code>src-tauri/src/translator.rs</code>).
            Files go to <code className="path">{status.dir}</code>.
          </p>
          <table className="downloads">
            <colgroup>
              <col style={{ width: 200 }} />
              <col />
              <col style={{ width: 80 }} />
              <col style={{ width: 90 }} />
            </colgroup>
            <thead>
              <tr><th>Part</th><th>File and SHA-256</th><th className="num">Size</th><th>License</th></tr>
            </thead>
            <tbody>
              {status.files.map((f) => (
                <tr key={f.url}>
                  <td>{f.part}</td>
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
