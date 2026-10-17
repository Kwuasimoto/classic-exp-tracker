/** Backdrops behind the panels (files in public/). Ellinia and Party are
 *  enlarged and anchored so their corner logo and bottom copyright band fall
 *  outside the window. */
export interface Backdrop {
  id: string;
  label: string;
  /** CSS background for the shell; `null` = plain ground colour. */
  css: { backgroundImage: string; backgroundPosition: string; backgroundSize: string } | null;
}

export const BACKDROPS: readonly Backdrop[] = [
  { id: "kerning", label: "Kerning City", css: { backgroundImage: "url(/maplestory-background-3.jpg)", backgroundPosition: "center", backgroundSize: "cover" } },
  { id: "ellinia", label: "Ellinia", css: { backgroundImage: "url(/maplestory-background-1.jpg)", backgroundPosition: "left bottom", backgroundSize: "auto 125%" } },
  { id: "party", label: "Party", css: { backgroundImage: "url(/maplestory-background-2.jpg)", backgroundPosition: "center top", backgroundSize: "auto 110%" } },
  { id: "none", label: "None", css: null },
];

const KEY = "backdrop";

/** The saved choice; the first backdrop if none is saved or storage is unavailable. */
export function savedBackdrop(): Backdrop {
  let id: string | null = null;
  try {
    id = localStorage.getItem(KEY);
  } catch {
    // Storage blocked: fall back to the default below.
  }
  return BACKDROPS.find((b) => b.id === id) ?? BACKDROPS[0]!;
}

export function saveBackdrop(id: string): void {
  try {
    localStorage.setItem(KEY, id);
  } catch {
    // Storage blocked: the choice lasts for this run only.
  }
}
