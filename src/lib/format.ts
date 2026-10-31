import { currentLocale, messages } from "./i18n";
import type { ReaderStatus } from "./ipc";
import type { Messages } from "../locales/en";

// Numbers read the same in every language, as the game shows them: 26,000.
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
  return h > 0 ? messages().time.hoursMinutes(h, m) : messages().time.minutes(m);
}

/** Dates and times read in the app's language. */
export const formatClock = (unix: number): string =>
  new Date(unix * 1000).toLocaleTimeString(currentLocale(), { hour: "numeric", minute: "2-digit" });

/** A snapshot's period on one line, written the way the app's language writes a range:
 *  `Oct 23, 1:09 – 3:09 PM`, across midnight `Oct 22, 10:20 PM – Oct 23, 1:12 AM`. */
export function formatPeriod(from: number, to: number): string {
  const format = new Intl.DateTimeFormat(currentLocale(), { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  return format.formatRange(new Date(from * 1000), new Date(to * 1000));
}

/** A file size: `16.7 MB`, `1.12 GB`. */
export function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

/** A day: `Oct 26`. */
export const formatDay = (unix: number): string => new Date(unix * 1000).toLocaleDateString(currentLocale(), { month: "short", day: "numeric" });

export const formatDateTime = (unix: number): string =>
  new Date(unix * 1000).toLocaleString(currentLocale(), { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** The status in a few words, for the page headers (statusText has the full explanation). */
export const statusShort = (s: ReaderStatus, m: Messages): string => m.status[s.kind];

export function statusText(s: ReaderStatus, m: Messages): string {
  const t = m.statusLong;
  switch (s.kind) {
    case "screenNotSupported":
      return t.screenNotSupported(s.detail);
    case "captureFailed":
      return t.captureFailed(s.detail);
    case "storageFailed":
      return t.storageFailed(s.detail);
    case "notifyFailed":
      return t.notifyFailed(s.detail);
    default:
      return t[s.kind];
  }
}
