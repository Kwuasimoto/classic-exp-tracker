import { availableMonitors, getCurrentWindow, PhysicalPosition, PhysicalSize, primaryMonitor } from "@tauri-apps/api/window";
import { err, ok, type Result } from "../lib/result";

const POSITION = "chat-popout-position";
const SIZE = "chat-popout-size";
/** The game's chat box at 1366x768 (read::chat): its 9 lines and scrollbar. The game fills
 *  the primary monitor, so on another 16:9 screen the box scales with it. */
const GAME = { width: 1366, height: 768 };
const CHAT_BOX = { x: 283, y: 548, width: 566, height: 150 };

function saved(key: string): [number, number] | null {
  let value: string | null = null;
  try {
    value = localStorage.getItem(key);
  } catch {
    // Storage blocked: the popout opens over the game's chat box.
  }
  const match = value?.match(/^(-?\d+),(-?\d+)$/);
  return match ? [Number(match[1]), Number(match[2])] : null;
}

function save(key: string, a: number, b: number): void {
  try {
    localStorage.setItem(key, `${a},${b}`);
  } catch {
    // Storage blocked: it opens over the game's chat box next time.
  }
}

/** Put the chat popout where and how big it was last left, if that's still on a monitor, else
 *  over the game's chat box on the primary monitor; then show it and remember where it's moved
 *  and how big it's made. */
export async function placeAndShow(): Promise<Result<void, string>> {
  const win = getCurrentWindow();
  try {
    const position = saved(POSITION);
    const size = saved(SIZE);
    const monitors = await availableMonitors();
    const onScreen = position !== null && monitors.some((m) =>
      position[0] >= m.position.x && position[0] < m.position.x + m.size.width && position[1] >= m.position.y && position[1] < m.position.y + m.size.height);
    if (onScreen && size !== null) {
      await win.setSize(new PhysicalSize(size[0], size[1]));
      await win.setPosition(new PhysicalPosition(position[0], position[1]));
    } else {
      const primary = await primaryMonitor();
      if (primary !== null) {
        const sx = primary.size.width / GAME.width;
        const sy = primary.size.height / GAME.height;
        await win.setSize(new PhysicalSize(Math.round(CHAT_BOX.width * sx), Math.round(CHAT_BOX.height * sy)));
        await win.setPosition(new PhysicalPosition(primary.position.x + Math.round(CHAT_BOX.x * sx), primary.position.y + Math.round(CHAT_BOX.y * sy)));
      }
    }
    await win.show();
    await win.onMoved(({ payload }) => save(POSITION, payload.x, payload.y));
    await win.onResized(({ payload }) => save(SIZE, payload.width, payload.height));
    return ok(undefined);
  } catch (e) {
    return err(String(e));
  }
}
