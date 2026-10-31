import type { ReaderStatus } from "./ipc";

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 });

export const formatExp = (n: number): string => compact.format(n);
/** A whole number with thousands separators: 26,000. */
export const formatWhole = (n: number): string => Math.round(n).toLocaleString("en-US");
export const formatPct = (bp: number): string => `${(bp / 100).toFixed(2)}%`;

export function formatHours(hours: number): string {
  if (!Number.isFinite(hours) || hours < 0) return "—";
  const total = Math.round(hours * 60);
  const h = Math.floor(total / 60);
  const m = total % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

export const formatClock = (unix: number): string =>
  new Date(unix * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** A snapshot's period on one line: `Oct 23 · 1:09–3:09 PM`; across midnight
 *  `Oct 22–23 · 10:20 PM–1:12 AM`. The start's AM/PM shows only when it differs from the end's. */
export function formatPeriod(from: number, to: number): string {
  const start = new Date(from * 1000);
  const end = new Date(to * 1000);
  const month = (d: Date) => d.toLocaleDateString([], { month: "short" });
  const time = (d: Date) => d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const sameHalf = start.getDate() === end.getDate() && start.getHours() < 12 === end.getHours() < 12;
  const startTime = sameHalf ? time(start).replace(/\s?[AP]M$/i, "") : time(start);
  const days =
    start.toDateString() === end.toDateString()
      ? `${month(start)} ${start.getDate()}`
      : month(start) === month(end)
        ? `${month(start)} ${start.getDate()}–${end.getDate()}`
        : `${month(start)} ${start.getDate()}–${month(end)} ${end.getDate()}`;
  return `${days} · ${startTime}–${time(end)}`;
}

/** A day: `Oct 26`. */
export const formatDay = (unix: number): string => new Date(unix * 1000).toLocaleDateString([], { month: "short", day: "numeric" });

export const formatDateTime = (unix: number): string =>
  new Date(unix * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** The status in a few words, for the page headers (statusText has the full explanation). */
export function statusShort(s: ReaderStatus): string {
  switch (s.kind) {
    case "ok":
      return "Reading OK";
    case "expUnreadable":
      return "EXP not readable";
    case "mapUnreadable":
      return "Map not readable";
    case "starting":
      return "Starting…";
    case "screenNotSupported":
      return "Screen isn't 16:9";
    case "captureFailed":
      return "Capture failed";
    case "storageFailed":
      return "Saving failed";
    case "notifyFailed":
      return "UI update failed";
  }
}

export function statusText(s: ReaderStatus): string {
  switch (s.kind) {
    case "ok":
      return "Reading OK";
    case "expUnreadable":
      return "EXP not readable — is the game full screen with the status bar visible?";
    case "mapUnreadable":
      return "Map name not readable — is the minimap open at the top-left?";
    case "starting":
      return "Starting screen capture…";
    case "screenNotSupported":
      return `${s.detail} isn't 16:9 — set Windows to a 16:9 resolution (1366×768 reads best) and play full screen`;
    case "captureFailed":
      return `Screen capture failed: ${s.detail}`;
    case "storageFailed":
      return `Saving failed: ${s.detail}`;
    case "notifyFailed":
      return `UI update failed: ${s.detail}`;
  }
}
