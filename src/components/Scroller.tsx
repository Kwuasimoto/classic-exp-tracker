import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

/** A box whose content scrolls without a scrollbar: its last visible line fades while more
 *  is below, so it reads as scrollable. While its content doesn't fit, it takes keyboard focus
 *  (the arrow keys scroll it) and `label` names it. */
export function Scroller({ className, label, children }: { className: string; label: string; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const measure = useCallback(() => {
    const el = box.current;
    if (!el) return;
    setMore(el.scrollHeight - el.scrollTop - el.clientHeight > 2);
    setOverflows(el.scrollHeight > el.clientHeight + 2);
  }, []);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
    measure();
    return () => observer.disconnect();
  }, [measure, children]);
  return (
    <div ref={box} className={more ? `${className} scroll-more` : className} onScroll={measure} role="region" aria-label={label} tabIndex={overflows ? 0 : undefined}>
      {children}
    </div>
  );
}
