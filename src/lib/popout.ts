import type { Popout } from "./ipc";

/** Whether each popout was open, remembered between runs so it reopens with the app. */
const KEY: Record<Popout, string> = { rates: "popout-open", chat: "chat-popout-open" };

export function savedPopoutOpen(popout: Popout): boolean {
  try {
    return localStorage.getItem(KEY[popout]) === "1";
  } catch {
    // Storage blocked: the popout starts closed.
    return false;
  }
}

export function savePopoutOpen(popout: Popout, open: boolean): void {
  try {
    localStorage.setItem(KEY[popout], open ? "1" : "0");
  } catch {
    // Storage blocked: the choice lasts for this run only.
  }
}
