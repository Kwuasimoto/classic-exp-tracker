import { availableMonitors, getCurrentWindow, LogicalSize, PhysicalPosition, primaryMonitor } from "@tauri-apps/api/window";
import { err, ok, type Result } from "../lib/result";

const KEY = "popout-position";
/** Gap from the top of the screen when the popout first opens (logical px). */
const TOP_GAP = 8;

function savedPosition(): PhysicalPosition | null {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(KEY);
  } catch {
    // Storage blocked: the popout opens in its default spot.
  }
  const match = saved?.match(/^(-?\d+),(-?\d+)$/);
  return match ? new PhysicalPosition(Number(match[1]), Number(match[2])) : null;
}

function savePosition(p: PhysicalPosition): void {
  try {
    localStorage.setItem(KEY, `${p.x},${p.y}`);
  } catch {
    // Storage blocked: the popout opens in its default spot next time.
  }
}

/** Put the popout where it was last left, if that's still on a monitor, else at the top centre
 *  of the primary monitor (the game's); then show it and remember where it's moved to. */
export async function placeAndShow(): Promise<Result<void, string>> {
  const win = getCurrentWindow();
  try {
    const saved = savedPosition();
    const monitors = await availableMonitors();
    const onScreen = saved !== null && monitors.some((m) =>
      saved.x >= m.position.x && saved.x < m.position.x + m.size.width && saved.y >= m.position.y && saved.y < m.position.y + m.size.height);
    let position = onScreen ? saved : null;
    if (position === null) {
      const primary = await primaryMonitor();
      if (primary !== null) {
        const size = await win.outerSize();
        position = new PhysicalPosition(
          primary.position.x + Math.round((primary.size.width - size.width) / 2),
          primary.position.y + Math.round(TOP_GAP * primary.scaleFactor),
        );
      }
    }
    if (position !== null) await win.setPosition(position);
    await win.show();
    await win.onMoved(({ payload }) => savePosition(payload));
    return ok(undefined);
  } catch (e) {
    return err(String(e));
  }
}

/** Fit the window to the popout's content width (the numbers grow with the rates). */
export async function fitWidth(width: number, height: number): Promise<Result<void, string>> {
  try {
    await getCurrentWindow().setSize(new LogicalSize(width, height));
    return ok(undefined);
  } catch (e) {
    return err(String(e));
  }
}
