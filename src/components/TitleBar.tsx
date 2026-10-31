import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The app's own title bar (the window has no native frame, see tauri.conf.json).
 *  `data-tauri-drag-region` makes the bar drag the window; double-click maximizes. `error` is
 *  a window error from elsewhere in the app (resizing for compressed mode), shown here too. */
export function TitleBar({ error: appError }: { error: string | null }) {
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Shown so a user can say which build they're running.
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion, (e: unknown) => setError(`version: ${String(e)}`));
  }, []);

  useEffect(() => {
    const win = getCurrentWindow();
    const refresh = () =>
      win.isMaximized().then(setMaximized, (e: unknown) => setError(`window state: ${String(e)}`));
    void refresh();
    let unlisten: (() => void) | null = null;
    let disposed = false;
    win.onResized(() => void refresh()).then(
      (u) => (disposed ? u() : (unlisten = u)),
      (e: unknown) => setError(`window events: ${String(e)}`),
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const run = (action: () => Promise<void>, what: string) => () =>
    void action().then(
      () => setError(null),
      (e: unknown) => setError(`${what}: ${String(e)}`),
    );
  const win = getCurrentWindow();

  return (
    <header className="titlebar" data-tauri-drag-region>
      <AppIcon />
      <span className="titlebar-name" data-tauri-drag-region>
        CLASSIC EXP TRACKER
      </span>
      {version && <span className="caption titlebar-version" data-tauri-drag-region>v{version}</span>}
      {(error ?? appError) && <span className="error titlebar-error" data-tauri-drag-region>{error ?? appError}</span>}
      <div className="window-controls">
        <button aria-label="Minimize" onClick={run(() => win.minimize(), "minimize")}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" stroke="currentColor" /></svg>
        </button>
        <button aria-label={maximized ? "Restore" : "Maximize"} onClick={run(() => win.toggleMaximize(), "maximize")}>
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
              <path d="M2.5 2.5V0.5h7v7h-2" stroke="currentColor" />
              <rect x="0.5" y="2.5" width="7" height="7" stroke="currentColor" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" /></svg>
          )}
        </button>
        <button className="close" aria-label="Close" onClick={run(() => win.close(), "close")}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
        </button>
      </div>
    </header>
  );
}

/** Three bars growing from small to large: the app icon (also src-tauri/icons). */
export function AppIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" data-tauri-drag-region>
      <rect x="2" y="14" width="5" height="8" rx="1.5" fill="#ffffff" />
      <rect x="9.5" y="8" width="5" height="14" rx="1.5" fill="#ffffff" />
      <rect x="17" y="2" width="5" height="20" rx="1.5" fill="var(--accent)" />
    </svg>
  );
}
