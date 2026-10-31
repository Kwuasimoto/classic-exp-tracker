/** Compressed (no charts, smaller type) or expanded (charts) layout, remembered between runs. */
export type Layout = "expanded" | "compressed";

const KEY = "layout";

/** The saved layout; expanded if none is saved or storage is unavailable. */
export function savedLayout(): Layout {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(KEY);
  } catch {
    // Storage blocked: fall back to the default below.
  }
  return saved === "compressed" ? "compressed" : "expanded";
}

export function saveLayout(layout: Layout): void {
  try {
    localStorage.setItem(KEY, layout);
  } catch {
    // Storage blocked: the choice lasts for this run only.
  }
}
