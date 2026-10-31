import { statusShort, statusText } from "../lib/format";
import { useMessages } from "../lib/i18n";
import type { AppError, ReaderStatus as Status } from "../lib/ipc";

/** The reader's status as a dot and a few words, with the full explanation on hover. It sits in
 *  every page's header, so a problem is always in sight. */
export function ReaderStatus({ status, error }: { status: Status | null; error: AppError | null }) {
  const m = useMessages();
  const ok = error === null && status?.kind === "ok";
  const long = error ? error.message : status ? statusText(status, m) : m.statusLong.starting;
  const short = error ? m.status.appError : status ? statusShort(status, m) : m.status.starting;
  return (
    <span className={ok ? "reader-status ok" : "reader-status"} role="status" title={long}>
      {short}
    </span>
  );
}
