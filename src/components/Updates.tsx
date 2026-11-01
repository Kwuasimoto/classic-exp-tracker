import { useEffect, useRef, useState } from "react";
import { formatClock } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { checkForUpdates, installUpdate, setUpdateChecks } from "../lib/ipc";
import { useUpdates } from "../lib/useUpdates";

/** The title bar's version, which opens what updates stand at (Check now, and the hourly
 *  checks on or off), and once a newer version is out, its Update button: only that click
 *  downloads, checks and installs it. The panel closes on a click outside or Escape. */
export function Updates({ version }: { version: string }) {
  const m = useMessages();
  const { updates, error, take } = useUpdates();
  const [open, setOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === "Escape" : !box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", close);
    };
  }, [open]);

  const install = () =>
    void installUpdate().then((res) => setActionError(res.success ? null : m.updates.installFailed(res.error.message)));
  const check = () => {
    setActionError(null);
    void checkForUpdates().then(take);
  };
  const setHourly = (on: boolean) =>
    void setUpdateChecks(on).then((res) => setActionError(res.success ? null : res.error.message));

  const line = !updates
    ? null
    : updates.checking
      ? m.updates.checking
      : updates.installing
        ? m.updates.installing
      : updates.available
        ? m.updates.available(updates.available)
        : updates.error
          ? m.updates.failed(updates.error)
          : updates.checkedAt !== null
            ? m.updates.upToDate(formatClock(updates.checkedAt))
            : m.updates.notChecked;
  const problem = actionError ?? error?.message ?? null;

  return (
    <div className="updates" ref={box}>
      <button className="caption titlebar-version" title={m.updates.title} aria-expanded={open} onClick={() => setOpen(!open)}>
        v{version}
      </button>
      {updates?.available && (
        <button className="btn small primary update-ready" title={m.updates.restartTitle} disabled={updates.installing} onClick={install}>
          {m.updates.updateTo(updates.available)}
        </button>
      )}
      {open && (
        <div className="flyout update-panel" role="dialog" aria-label={m.updates.title}>
          {line && <p className={updates?.error && !updates.checking && !updates.installing && !updates.available ? "error" : undefined} role="status">{line}</p>}
          {problem && <p className="error">{problem}</p>}
          <div className="update-actions">
            {updates?.available && (
              <button className="btn small primary" title={m.updates.restartTitle} disabled={updates.installing} onClick={install}>
                {m.updates.updateTo(updates.available)}
              </button>
            )}
            <button className="btn small" disabled={!updates || updates.checking || updates.installing} onClick={check}>
              {m.updates.checkNow}
            </button>
          </div>
          <label className="update-hourly">
            <input type="checkbox" checked={updates?.autoCheck ?? true} disabled={!updates} onChange={(e) => setHourly(e.target.checked)} />
            {m.updates.hourly}
          </label>
        </div>
      )}
    </div>
  );
}
