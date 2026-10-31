/** Whether the popout was open, remembered between runs so it reopens with the app. */
const KEY = "popout-open";

export function savedPopoutOpen(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    // Storage blocked: the popout starts closed.
    return false;
  }
}

export function savePopoutOpen(open: boolean): void {
  try {
    localStorage.setItem(KEY, open ? "1" : "0");
  } catch {
    // Storage blocked: the choice lasts for this run only.
  }
}
