import { useState } from "react";

const PAGE_SIZE = 10;

/** One page of `items`, and the Previous/Next controls for it (none for a single page).
 *  If the list shrinks (a snapshot deleted), the page clamps to the last one. */
export function usePages<T>(items: T[]) {
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(items.length / PAGE_SIZE));
  const current = Math.min(page, pages - 1);
  const rows = items.slice(current * PAGE_SIZE, (current + 1) * PAGE_SIZE);
  const pager = pages > 1 && (
    <div className="row pager">
      <button className="btn" disabled={current === 0} onClick={() => setPage(current - 1)}>Previous</button>
      <span className="caption">Page {current + 1} of {pages}</span>
      <button className="btn" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>Next</button>
    </div>
  );
  return { rows, pager };
}
