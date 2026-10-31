import { useState } from "react";
import { useMessages } from "../lib/i18n";

const PAGE_SIZE = 10;

/** One page of `items`, and the Previous/Next controls for it (none for a single page).
 *  If the list shrinks (a snapshot deleted), the page clamps to the last one. */
export function usePages<T>(items: T[]) {
  const m = useMessages();
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(items.length / PAGE_SIZE));
  const current = Math.min(page, pages - 1);
  const rows = items.slice(current * PAGE_SIZE, (current + 1) * PAGE_SIZE);
  const pager = pages > 1 && (
    <div className="row pager">
      <button className="btn" disabled={current === 0} onClick={() => setPage(current - 1)}>{m.common.previous}</button>
      <span className="caption">{m.common.page(current + 1, pages)}</span>
      <button className="btn" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>{m.common.next}</button>
    </div>
  );
  return { rows, pager };
}
