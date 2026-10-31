import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

/** A box whose content scrolls without a scrollbar: its last visible line fades while more
 *  is below, so it reads as scrollable. */
export function Scroller({ className, children }: { className: string; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  const measure = useCallback(() => {
    const el = box.current;
    if (el) setMore(el.scrollHeight - el.scrollTop - el.clientHeight > 2);
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
    <div ref={box} className={more ? `${className} scroll-more` : className} onScroll={measure}>
      {children}
    </div>
  );
}
