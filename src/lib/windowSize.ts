import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { err, ok, type Result } from "./result";

/** A window size in logical (CSS) pixels. */
export interface WindowSize {
  width: number;
  height: number;
}

const KEY = "expanded-size";
/** The window's size in tauri.conf.json: what expanding goes back to when none is saved. */
const DEFAULT_EXPANDED: WindowSize = { width: 1100, height: 760 };

/** The size the window had before it was compressed; the default if none is saved or storage is unavailable. */
export function savedExpandedSize(): WindowSize {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(KEY);
  } catch {
    // Storage blocked: fall back to the default below.
  }
  const match = saved?.match(/^(\d+)x(\d+)$/);
  if (!match) return DEFAULT_EXPANDED;
  const width = Number(match[1]);
  const height = Number(match[2]);
  return width > 0 && height > 0 ? { width, height } : DEFAULT_EXPANDED;
}

export function saveExpandedSize(size: WindowSize): void {
  try {
    localStorage.setItem(KEY, `${Math.round(size.width)}x${Math.round(size.height)}`);
  } catch {
    // Storage blocked: expanding goes back to the default size.
  }
}

/** The window's current size (the page fills the window: it has no native frame). */
export function currentSize(): WindowSize {
  return { width: window.innerWidth, height: window.innerHeight };
}

/** The compressed window: the rail and the ledger column wide (the shell's padding and gap
 *  included), never wider than `limit`, and as tall as `limit` (the expanded window). */
export function compressedSize(root: HTMLElement, limit: WindowSize): Result<WindowSize, string> {
  const shell = root.querySelector<HTMLElement>(".shell");
  const rail = root.querySelector<HTMLElement>(".rail");
  if (!shell || !rail) return err("the page isn't laid out yet");
  const px = (value: string) => parseFloat(value) || 0;
  const shellStyle = getComputedStyle(shell);
  const ledgerWidth = px(getComputedStyle(root).getPropertyValue("--ledger-width"));
  const width = Math.ceil(px(shellStyle.paddingLeft) + rail.offsetWidth + px(shellStyle.columnGap) + ledgerWidth + px(shellStyle.paddingRight));
  return ok({ width: Math.min(width, limit.width), height: limit.height });
}

/** Resize the window (out of maximized first, where a size doesn't apply). */
export async function resizeWindow(size: WindowSize): Promise<Result<void, string>> {
  const win = getCurrentWindow();
  try {
    if (await win.isMaximized()) await win.unmaximize();
    await win.setSize(new LogicalSize(size.width, size.height));
    return ok(undefined);
  } catch (e) {
    return err(String(e));
  }
}
