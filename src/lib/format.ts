import type { ReaderStatus } from "./ipc";

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 });

export const formatExp = (n: number): string => compact.format(n);
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

export const formatDateTime = (unix: number): string =>
  new Date(unix * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

export function statusText(s: ReaderStatus): string {
  switch (s.kind) {
    case "ok":
      return "Reading OK";
    case "expUnreadable":
      return "EXP not readable — is the status bar visible?";
    case "mapUnreadable":
      return "Map name not readable — is the minimap open at the top-left?";
    case "notCalibrated":
      return "Not calibrated for this screen resolution";
    case "captureFailed":
      return `Screen capture failed: ${s.detail}`;
    case "storageFailed":
      return `Saving failed: ${s.detail}`;
    case "notifyFailed":
      return `UI update failed: ${s.detail}`;
  }
}
